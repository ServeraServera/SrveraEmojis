//! Pasting: put the text on the clipboard + primary selection through
//! ext-data-control, then press Shift+Insert (or Ctrl+V) on a virtual uinput keyboard.
//! Picks are marked secret so clipboard history skips them, and afterwards the
//! text that was on the clipboard before is put back.

use crate::app::{App, Qh};
use std::io::{Read, Write};
use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};
use std::sync::Arc;
use wayland_client::{event_created_child, Connection, Dispatch};
use wayland_protocols::ext::data_control::v1::client::{
    ext_data_control_device_v1::{self, ExtDataControlDeviceV1},
    ext_data_control_manager_v1::ExtDataControlManagerV1,
    ext_data_control_offer_v1::ExtDataControlOfferV1,
    ext_data_control_source_v1::{self, ExtDataControlSourceV1},
};

const MIMES: &[&str] = &["text/plain;charset=utf-8", "text/plain", "UTF8_STRING", "STRING", "TEXT"];
/// Klipper (and other clipboard managers) don't record entries offering this as "secret".
const HINT: &str = "x-kde-passwordManagerHint";

// ---------------------------------------------------------------- clipboard

#[derive(Default)]
pub struct Clipboard {
    pub manager: Option<ExtDataControlManagerV1>,
    pub device: Option<ExtDataControlDeviceV1>,
    sel_offer: Option<ExtDataControlOfferV1>,
    prim_offer: Option<ExtDataControlOfferV1>,
    // [clipboard, primary]: our source that currently owns each one, and the text before our picks.
    cur_src: [Option<ExtDataControlSourceV1>; 2],
    saved: [Option<String>; 2],
}

impl Clipboard {
    pub fn new(manager: Option<ExtDataControlManagerV1>) -> Self {
        Clipboard { manager, ..Default::default() }
    }

    fn source(&self, text: &str, qh: &Qh) -> Option<ExtDataControlSourceV1> {
        let src = self.manager.as_ref()?.create_data_source(qh, Arc::from(text));
        for mime in MIMES.iter().chain([&HINT]) {
            src.offer(mime.to_string());
        }
        Some(src)
    }

    /// Offers `text` as both the clipboard and the primary selection,
    /// remembering the previous clipboard and primary text first.
    pub fn set(&mut self, text: &str, qh: &Qh, conn: &Connection) -> bool {
        let Some(d) = self.device.clone() else { return false };
        for (i, offer) in [&self.sel_offer, &self.prim_offer].into_iter().enumerate() {
            if self.cur_src[i].is_none() {
                self.saved[i] = offer.as_ref().and_then(|o| read_text(o, conn));
            }
        }
        let (Some(a), Some(b)) = (self.source(text, qh), self.source(text, qh)) else { return false };
        d.set_selection(Some(&a));
        d.set_primary_selection(Some(&b));
        self.cur_src = [Some(a), Some(b)];
        true
    }

    /// True while both the clipboard and the primary selection still hold our pick.
    pub fn still_ours(&self) -> bool {
        self.cur_src.iter().all(Option::is_some)
    }

    /// After the paste: put the previous texts back (or empty them). Each one is
    /// skipped if something else took it meanwhile, so that copy isn't overwritten.
    pub fn restore(&mut self, qh: &Qh) {
        let Some(d) = self.device.clone() else { return };
        for i in 0..2 {
            if self.cur_src[i].is_none() {
                continue;
            }
            let src = self.saved[i].as_deref().and_then(|t| self.source(t, qh));
            if i == 0 {
                d.set_selection(src.as_ref());
            } else {
                d.set_primary_selection(src.as_ref());
            }
            self.cur_src[i] = src;
        }
    }
}

/// Reads the text of a clipboard offer, giving up after 300 ms of silence.
fn read_text(offer: &ExtDataControlOfferV1, conn: &Connection) -> Option<String> {
    let (mut r, w) = std::io::pipe().ok()?;
    offer.receive(MIMES[0].to_string(), w.as_fd());
    drop(w);
    conn.flush().ok()?;
    let (mut out, mut buf) = (Vec::new(), [0u8; 4096]);
    loop {
        let mut pfd = libc::pollfd { fd: r.as_raw_fd(), events: libc::POLLIN, revents: 0 };
        if unsafe { libc::poll(&mut pfd, 1, 300) } <= 0 || out.len() > 4 << 20 {
            return None;
        }
        match r.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => out.extend_from_slice(&buf[..n]),
            Err(_) => return None,
        }
    }
    String::from_utf8(out).ok().filter(|s| !s.is_empty())
}

ignore_events!(ExtDataControlManagerV1, ExtDataControlOfferV1);

impl Dispatch<ExtDataControlSourceV1, Arc<str>> for App {
    fn event(
        app: &mut Self,
        src: &ExtDataControlSourceV1,
        ev: ext_data_control_source_v1::Event,
        text: &Arc<str>,
        _: &Connection,
        _: &Qh,
    ) {
        match ev {
            ext_data_control_source_v1::Event::Send { mime_type, fd } => {
                let data = if mime_type == HINT { "secret" } else { text };
                let _ = std::fs::File::from(fd).write_all(data.as_bytes());
            }
            ext_data_control_source_v1::Event::Cancelled => {
                // Someone else copied or selected something: that slot is no longer ours.
                for s in &mut app.clipboard.cur_src {
                    if s.as_ref() == Some(src) {
                        *s = None;
                    }
                }
                src.destroy();
            }
            _ => {}
        }
    }
}

impl Dispatch<ExtDataControlDeviceV1, ()> for App {
    fn event(
        app: &mut Self,
        _: &ExtDataControlDeviceV1,
        ev: ext_data_control_device_v1::Event,
        _: &(),
        _: &Connection,
        _: &Qh,
    ) {
        // Keep the latest offers (to read the old text before a pick); free replaced ones.
        use ext_data_control_device_v1::Event;
        let c = &mut app.clipboard;
        let (slot, id) = match ev {
            Event::Selection { id } => (&mut c.sel_offer, id),
            Event::PrimarySelection { id } => (&mut c.prim_offer, id),
            Event::Finished => {
                c.device = None;
                return;
            }
            _ => return,
        };
        if let Some(old) = std::mem::replace(slot, id) {
            old.destroy();
        }
    }

    event_created_child!(App, ExtDataControlDeviceV1, [
        ext_data_control_device_v1::EVT_DATA_OFFER_OPCODE => (ExtDataControlOfferV1, ()),
    ]);
}

// ---------------------------------------------------------------- uinput keyboard

const EV_SYN: u16 = 0;
const EV_KEY: u16 = 1;
const KEY_LEFTCTRL: u16 = 29;
const KEY_LEFTSHIFT: u16 = 42;
const KEY_V: u16 = 47;
const KEY_INSERT: u16 = 110;
const UI_SET_EVBIT: libc::c_ulong = 0x4004_5564;
const UI_SET_KEYBIT: libc::c_ulong = 0x4004_5565;
const UI_DEV_SETUP: libc::c_ulong = 0x405c_5503;
const UI_DEV_CREATE: libc::c_ulong = 0x5501;

#[repr(C)]
struct UinputSetup {
    bustype: u16,
    vendor: u16,
    product: u16,
    version: u16,
    name: [u8; 80],
    ff_effects_max: u32,
}

#[repr(C)]
struct InputEvent {
    time: libc::timeval,
    kind: u16,
    code: u16,
    value: i32,
}

#[derive(Clone, Copy, PartialEq)]
pub enum PasteKey {
    ShiftInsert,
    CtrlV,
}

/// A virtual keyboard that only knows the keys it needs. Created once at startup
/// so the compositor has already picked it up by the time we paste.
pub struct Keyboard {
    fd: OwnedFd,
}

impl Keyboard {
    pub fn new() -> std::io::Result<Self> {
        let raw = unsafe { libc::open(c"/dev/uinput".as_ptr(), libc::O_WRONLY | libc::O_NONBLOCK | libc::O_CLOEXEC) };
        if raw < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let fd = unsafe { OwnedFd::from_raw_fd(raw) };
        let check = |r: libc::c_int| if r < 0 { Err(std::io::Error::last_os_error()) } else { Ok(()) };
        unsafe {
            check(libc::ioctl(raw, UI_SET_EVBIT, EV_KEY as libc::c_int))?;
            for k in [KEY_LEFTCTRL, KEY_LEFTSHIFT, KEY_V, KEY_INSERT] {
                check(libc::ioctl(raw, UI_SET_KEYBIT, k as libc::c_int))?;
            }
            let mut setup = UinputSetup {
                bustype: 0x06, /* BUS_VIRTUAL */
                vendor: 0x1,
                product: 0x1,
                version: 1,
                name: [0; 80],
                ff_effects_max: 0,
            };
            let name = b"srvera-emoji virtual keyboard";
            setup.name[..name.len()].copy_from_slice(name);
            check(libc::ioctl(raw, UI_DEV_SETUP, &setup))?;
            check(libc::ioctl(raw, UI_DEV_CREATE))?;
        }
        Ok(Keyboard { fd })
    }

    fn emit(&self, kind: u16, code: u16, value: i32) {
        let ev = InputEvent { time: libc::timeval { tv_sec: 0, tv_usec: 0 }, kind, code, value };
        unsafe {
            libc::write(self.fd.as_raw_fd(), &ev as *const _ as *const libc::c_void, std::mem::size_of::<InputEvent>());
        }
    }

    fn key(&self, code: u16, down: bool) {
        self.emit(EV_KEY, code, down as i32);
        self.emit(EV_SYN, 0, 0);
        std::thread::sleep(std::time::Duration::from_millis(4));
    }

    pub fn paste(&self, which: PasteKey) {
        let (m, k) = match which {
            PasteKey::ShiftInsert => (KEY_LEFTSHIFT, KEY_INSERT),
            PasteKey::CtrlV => (KEY_LEFTCTRL, KEY_V),
        };
        self.key(m, true);
        self.key(k, true);
        self.key(k, false);
        self.key(m, false);
    }
}
