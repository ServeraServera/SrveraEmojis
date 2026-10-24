//! Pasting: put the text on the clipboard + primary selection through
//! ext-data-control, then press Shift+Insert (or Ctrl+V) on a virtual uinput keyboard.

use crate::app::App;
use std::io::Write;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use wayland_client::{event_created_child, Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols::ext::data_control::v1::client::{
    ext_data_control_device_v1::{self, ExtDataControlDeviceV1},
    ext_data_control_manager_v1::ExtDataControlManagerV1,
    ext_data_control_offer_v1::ExtDataControlOfferV1,
    ext_data_control_source_v1::{self, ExtDataControlSourceV1},
};

const MIMES: &[&str] = &["text/plain;charset=utf-8", "text/plain", "UTF8_STRING", "STRING", "TEXT"];

// ---------------------------------------------------------------- clipboard

#[derive(Default)]
pub struct Clipboard {
    pub manager: Option<ExtDataControlManagerV1>,
    pub device: Option<ExtDataControlDeviceV1>,
    text: String,
    sel_offer: Option<ExtDataControlOfferV1>,
    prim_offer: Option<ExtDataControlOfferV1>,
}

impl Clipboard {
    pub fn new(manager: Option<ExtDataControlManagerV1>) -> Self {
        Clipboard { manager, ..Default::default() }
    }

    /// Offers `text` as both the clipboard and the primary selection.
    pub fn set(&mut self, text: &str, qh: &QueueHandle<App>) -> bool {
        let (Some(m), Some(d)) = (&self.manager, &self.device) else { return false };
        self.text = text.to_string();
        for primary in [false, true] {
            let src = m.create_data_source(qh, ());
            for mime in MIMES {
                src.offer(mime.to_string());
            }
            if primary {
                d.set_primary_selection(Some(&src));
            } else {
                d.set_selection(Some(&src));
            }
        }
        true
    }
}

impl Dispatch<ExtDataControlManagerV1, ()> for App {
    fn event(_: &mut Self, _: &ExtDataControlManagerV1, _: <ExtDataControlManagerV1 as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<ExtDataControlSourceV1, ()> for App {
    fn event(app: &mut Self, src: &ExtDataControlSourceV1, ev: ext_data_control_source_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        match ev {
            ext_data_control_source_v1::Event::Send { fd, .. } => {
                let mut f = std::fs::File::from(fd);
                let _ = f.write_all(app.clipboard.text.as_bytes());
            }
            ext_data_control_source_v1::Event::Cancelled => src.destroy(),
            _ => {}
        }
    }
}

impl Dispatch<ExtDataControlDeviceV1, ()> for App {
    fn event(app: &mut Self, _: &ExtDataControlDeviceV1, ev: ext_data_control_device_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        // We never read other apps' clipboards; just free their offers once replaced.
        use ext_data_control_device_v1::Event;
        let c = &mut app.clipboard;
        match ev {
            Event::Selection { id } => {
                if let Some(old) = std::mem::replace(&mut c.sel_offer, id) {
                    old.destroy();
                }
            }
            Event::PrimarySelection { id } => {
                if let Some(old) = std::mem::replace(&mut c.prim_offer, id) {
                    old.destroy();
                }
            }
            Event::Finished => c.device = None,
            _ => {}
        }
    }

    event_created_child!(App, ExtDataControlDeviceV1, [
        ext_data_control_device_v1::EVT_DATA_OFFER_OPCODE => (ExtDataControlOfferV1, ()),
    ]);
}

impl Dispatch<ExtDataControlOfferV1, ()> for App {
    fn event(_: &mut Self, _: &ExtDataControlOfferV1, _: <ExtDataControlOfferV1 as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
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
            let mut setup = UinputSetup { bustype: 0x06 /* BUS_VIRTUAL */, vendor: 0x1, product: 0x1, version: 1, name: [0; 80], ff_effects_max: 0 };
            let name = b"emoji-picker virtual keyboard";
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
