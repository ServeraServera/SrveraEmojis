//! Global shortcut through KDE's kglobalaccel over D-Bus, so Super+. reaches the running
//! daemon directly instead of making KDE start a new process from disk on every press.
//! A minimal D-Bus client: only the few message types and calls this needs.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;

const KGA: &str = "org.kde.kglobalaccel";
/// (component, action, component name, action name) as shown in System Settings → Shortcuts.
const ID: [&str; 4] = ["srvera-emoji", "toggle", "Srvera Emoji", "Show or hide the picker"];
const META_PERIOD: u32 = 0x1000_002E; // Qt::META | Qt::Key_Period
const SET_PRESENT: u32 = 2;
const IS_DEFAULT: u32 = 8;
const NO_REPLY: u8 = 1;

pub enum Event {
    Toggle,
    Reregister, // kglobalaccel (KWin) started again
}

pub struct Bus {
    pub sock: UnixStream,
    serial: u32,
    present_call: u32, // serial of the call whose reply holds the keys in use
    owners_call: u32,  // serial of the "who else uses this key" query
    buf: Vec<u8>,
}

/// D-Bus little-endian writer; alignment is relative to the start of the buffer.
struct W(Vec<u8>);

impl W {
    fn pad(&mut self, n: usize) {
        self.0.resize(self.0.len().next_multiple_of(n), 0);
    }
    fn u32(&mut self, v: u32) {
        self.pad(4);
        self.0.extend(v.to_le_bytes());
    }
    fn str(&mut self, s: &str) {
        self.u32(s.len() as u32);
        self.0.extend(s.as_bytes());
        self.0.push(0);
    }
    fn sig(&mut self, s: &str) {
        self.0.push(s.len() as u8);
        self.0.extend(s.as_bytes());
        self.0.push(0);
    }
    /// An array whose elements are aligned to `align`; its byte length is patched in after.
    fn array(&mut self, align: usize, f: impl FnOnce(&mut W)) {
        self.u32(0);
        let at = self.0.len() - 4;
        self.pad(align);
        let start = self.0.len();
        f(self);
        let n = (self.0.len() - start) as u32;
        self.0[at..at + 4].copy_from_slice(&n.to_le_bytes());
    }
}

fn session_bus_path() -> Option<String> {
    let addr = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap_or_default();
    let path =
        addr.split(';').find_map(|a| a.strip_prefix("unix:")?.split(',').find_map(|kv| kv.strip_prefix("path=")));
    path.map(String::from).or_else(|| Some(format!("{}/bus", std::env::var("XDG_RUNTIME_DIR").ok()?)))
}

impl Bus {
    /// Connects and authenticates to the session bus.
    pub fn connect() -> std::io::Result<Self> {
        let path = session_bus_path().ok_or(std::io::ErrorKind::NotFound)?;
        let mut sock = UnixStream::connect(path)?;
        sock.set_read_timeout(Some(std::time::Duration::from_secs(2)))?;
        let uid: String = unsafe { libc::getuid() }.to_string().bytes().map(|b| format!("{b:02x}")).collect();
        sock.write_all(format!("\0AUTH EXTERNAL {uid}\r\n").as_bytes())?;
        let mut line = [0u8; 128];
        let n = sock.read(&mut line)?;
        if !line[..n].starts_with(b"OK ") {
            return Err(std::io::Error::other("D-Bus authentication failed"));
        }
        sock.write_all(b"BEGIN\r\n")?;
        let mut bus = Bus { sock, serial: 0, present_call: 0, owners_call: 0, buf: Vec::new() };
        bus.call("org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus", "Hello", "", &[], 0)?;
        bus.sock.set_nonblocking(true)?;
        Ok(bus)
    }

    #[allow(clippy::too_many_arguments)]
    fn call(
        &mut self,
        dest: &str,
        path: &str,
        iface: &str,
        member: &str,
        sig: &str,
        body: &[u8],
        flags: u8,
    ) -> std::io::Result<u32> {
        self.serial += 1;
        let mut m = W(vec![b'l', 1, flags, 1]);
        m.u32(body.len() as u32);
        m.u32(self.serial);
        let fields = [(1, "o", path), (2, "s", iface), (3, "s", member), (6, "s", dest), (8, "g", sig)];
        m.array(8, |m| {
            for (code, t, v) in fields.into_iter().filter(|f| !f.2.is_empty()) {
                m.pad(8);
                m.0.push(code);
                m.sig(t);
                if t == "g" {
                    m.sig(v)
                } else {
                    m.str(v)
                }
            }
        });
        m.pad(8);
        m.0.extend(body);
        self.sock.write_all(&m.0)?;
        Ok(self.serial)
    }

    fn add_match(&mut self, rule: &str) -> std::io::Result<()> {
        let mut b = W(Vec::new());
        b.str(rule);
        self.call(
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "AddMatch",
            "s",
            &b.0,
            NO_REPLY,
        )?;
        Ok(())
    }

    /// Subscribes to the key press and to kglobalaccel restarts, then registers the shortcut.
    pub fn start(&mut self) -> std::io::Result<()> {
        self.add_match(&format!(
            "type='signal',interface='org.kde.kglobalaccel.Component',member='globalShortcutPressed',arg0='{}'",
            ID[0]
        ))?;
        self.add_match(&format!("type='signal',member='NameOwnerChanged',arg0='{KGA}'"))?;
        self.register()
    }

    /// Registers the action with Meta+. as its default. A key changed in System Settings
    /// is kept: kglobalaccel answers with the keys actually in use.
    pub fn register(&mut self) -> std::io::Result<()> {
        let id = |w: &mut W| w.array(4, |w| ID.iter().for_each(|s| w.str(s)));
        let mut b = W(Vec::new());
        id(&mut b);
        self.call(KGA, "/kglobalaccel", "org.kde.KGlobalAccel", "doRegister", "as", &b.0, NO_REPLY)?;
        for flags in [SET_PRESENT | IS_DEFAULT, SET_PRESENT] {
            let mut b = W(Vec::new());
            id(&mut b);
            b.array(4, |w| w.u32(META_PERIOD));
            b.u32(flags);
            let reply = if flags == SET_PRESENT { 0 } else { NO_REPLY };
            self.present_call =
                self.call(KGA, "/kglobalaccel", "org.kde.KGlobalAccel", "setShortcut", "asaiu", &b.0, reply)?;
        }
        Ok(())
    }

    /// Reads whatever arrived and returns what the app should do (None: the bus went away).
    pub fn read(&mut self) -> Option<Vec<Event>> {
        let mut chunk = [0u8; 4096];
        loop {
            match self.sock.read(&mut chunk) {
                Ok(0) => return None,
                Ok(n) => self.buf.extend_from_slice(&chunk[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(_) => return None,
            }
        }
        let mut events = Vec::new();
        while let Some(len) = msg_len(&self.buf) {
            if self.buf.len() < len {
                break;
            }
            let m: Vec<u8> = self.buf.drain(..len).collect();
            self.handle(&m, &mut events);
        }
        Some(events)
    }

    fn handle(&mut self, m: &[u8], events: &mut Vec<Event>) {
        let Some((kind, member, reply_to, body)) = parse(m) else { return };
        let mut p = 0;
        match (kind, member) {
            (4, "globalShortcutPressed") if body_str(body, &mut p) == ID[0] && body_str(body, &mut p) == ID[1] => {
                events.push(Event::Toggle)
            }
            (4, "NameOwnerChanged") if body_str(body, &mut p) == KGA => {
                body_str(body, &mut p);
                if !body_str(body, &mut p).is_empty() {
                    events.push(Event::Reregister);
                }
            }
            // The keys in use. kglobalaccel lets two actions list the same key, but only the
            // first one gets it, so ask who else has ours.
            (2, _) if reply_to == self.present_call => match rd_u32(body, 4).filter(|_| rd_u32(body, 0) != Some(0)) {
                Some(key) => {
                    let body = key.to_le_bytes();
                    let path = "/kglobalaccel";
                    let q = self.call(KGA, path, "org.kde.KGlobalAccel", "getGlobalShortcutsByKey", "i", &body, 0);
                    self.owners_call = q.unwrap_or(0);
                }
                None => eprintln!(
                    "srvera-emoji: no shortcut set; assign one to Srvera Emoji in System Settings → Shortcuts"
                ),
            },
            (2, _) if reply_to == self.owners_call => {
                for name in other_owners(body) {
                    eprintln!(
                        "srvera-emoji: our shortcut is also bound to \"{name}\", which keeps it; remove it there \
                         (System Settings → Shortcuts) or give Srvera Emoji another key, then restart srvera-emoji"
                    );
                }
            }
            (3, _) if reply_to == self.present_call => {
                eprintln!("srvera-emoji: could not register the global shortcut")
            }
            _ => {}
        }
    }
}

/// Friendly names of other components in a getGlobalShortcutsByKey reply, a(ssssssaiai):
/// (action, action name, component, component name, context, context name, keys, default keys).
fn other_owners(b: &[u8]) -> Vec<&str> {
    let (mut out, mut p) = (Vec::new(), 8);
    let end = 8 + rd_u32(b, 0).unwrap_or(0) as usize;
    while p < end.min(b.len()) {
        p = p.next_multiple_of(8);
        let s: Vec<&str> = (0..6).map(|_| body_str(b, &mut p)).collect();
        for _ in 0..2 {
            p = p.next_multiple_of(4) + 4 + rd_u32(b, p.next_multiple_of(4)).unwrap_or(0) as usize;
        }
        if s[2] != ID[0] {
            out.push(s[3]);
        }
    }
    out
}

fn rd_u32(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

/// Total length of the message at the start of `b`, once its fixed header is in.
fn msg_len(b: &[u8]) -> Option<usize> {
    let (body, fields) = (rd_u32(b, 4)? as usize, rd_u32(b, 12)? as usize);
    Some((16 + fields).next_multiple_of(8) + body)
}

/// A string at `*p` in a message body, advancing `*p` past it.
fn body_str<'a>(b: &'a [u8], p: &mut usize) -> &'a str {
    let o = p.next_multiple_of(4);
    let Some(len) = rd_u32(b, o) else { return "" };
    *p = o + 4 + len as usize + 1;
    b.get(o + 4..o + 4 + len as usize).and_then(|s| std::str::from_utf8(s).ok()).unwrap_or("")
}

/// (message type, member, reply serial, body) of a little-endian message.
fn parse(m: &[u8]) -> Option<(u8, &str, u32, &[u8])> {
    if m[0] != b'l' {
        return None;
    }
    let end = 16 + rd_u32(m, 12)? as usize;
    let (mut p, mut member, mut reply_to) = (16, "", 0);
    while p < end {
        p = p.next_multiple_of(8);
        let (code, sig_len) = (*m.get(p)?, *m.get(p + 1)? as usize);
        let t = *m.get(p + 2)?;
        p += 3 + sig_len;
        match t {
            b's' | b'o' => {
                let s = body_str(m, &mut p);
                if code == 3 {
                    member = s;
                }
            }
            b'g' => p += *m.get(p)? as usize + 2,
            b'u' => {
                p = p.next_multiple_of(4);
                if code == 5 {
                    reply_to = rd_u32(m, p)?;
                }
                p += 4;
            }
            _ => return None,
        }
    }
    Some((m[1], member, reply_to, m.get(end.next_multiple_of(8)..)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A signal as kglobalaccel sends it, built with our own writer.
    fn signal(member: &str, strs: &[&str]) -> Vec<u8> {
        let mut body = W(Vec::new());
        strs.iter().for_each(|s| body.str(s));
        let mut m = W(vec![b'l', 4, 0, 1]);
        m.u32(body.0.len() as u32);
        m.u32(7);
        m.array(8, |m| {
            for (code, v) in [(1, "/component/srvera_emoji"), (3, member)] {
                m.pad(8);
                m.0.push(code);
                m.sig(if code == 1 { "o" } else { "s" });
                m.str(v);
            }
        });
        m.pad(8);
        m.0.extend(body.0);
        m.0
    }

    fn events(bytes: &[u8]) -> usize {
        let mut bus =
            Bus { sock: UnixStream::pair().unwrap().0, serial: 0, present_call: 99, owners_call: 98, buf: Vec::new() };
        let mut ev = Vec::new();
        assert_eq!(msg_len(bytes), Some(bytes.len()));
        bus.handle(bytes, &mut ev);
        ev.len()
    }

    #[test]
    fn shortcut_signal() {
        assert_eq!(events(&signal("globalShortcutPressed", &["srvera-emoji", "toggle"])), 1);
        assert_eq!(events(&signal("globalShortcutPressed", &["srvera-emoji", "other"])), 0);
        assert_eq!(events(&signal("globalShortcutReleased", &["srvera-emoji", "toggle"])), 0);
        assert_eq!(events(&signal("NameOwnerChanged", &[KGA, "", ":1.5"])), 1);
        assert_eq!(events(&signal("NameOwnerChanged", &[KGA, ":1.5", ""])), 0);
    }
}
