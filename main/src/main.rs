//! srvera-emoji: a tiny background emoji & kaomoji picker for Wayland.
//!
//!   srvera-emoji            start the background daemon (hidden)
//!   srvera-emoji --toggle   show/hide the picker (starts the daemon if needed)
//!   srvera-emoji --show     show the picker
//!   srvera-emoji --quit     stop the daemon
//!   --paste-key ctrl-v      paste with Ctrl+V instead of Shift+Insert

/// Empty `Dispatch` impls for protocol objects whose events we don't need.
macro_rules! ignore_events {
    ($($t:ty),*) => {$(
        impl wayland_client::Dispatch<$t, ()> for crate::app::App {
            fn event(_: &mut Self, _: &$t, _: <$t as wayland_client::Proxy>::Event, _: &(), _: &wayland_client::Connection, _: &crate::app::Qh) {}
        }
    )*};
}

mod app;
mod data;
mod emoji_img;
mod paste;
mod text;
mod ui;

use std::io::Write;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;

fn socket_path() -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    dir.join("srvera-emoji.sock")
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let has = |f: &str| args.iter().any(|a| a == f);
    if has("-h") || has("--help") {
        println!("usage: srvera-emoji [--toggle | --show | --quit] [--paste-key shift-insert|ctrl-v]");
        return;
    }
    let paste_key = match args.iter().position(|a| a == "--paste-key").and_then(|i| args.get(i + 1)) {
        Some(k) if k == "ctrl-v" => paste::PasteKey::CtrlV,
        _ => paste::PasteKey::ShiftInsert,
    };
    let cmd = [("--toggle", b't'), ("--show", b's'), ("--quit", b'q')].into_iter().find(|f| has(f.0)).map(|f| f.1);

    let path = socket_path();
    if let Ok(mut s) = UnixStream::connect(&path) {
        match cmd {
            Some(c) => {
                let _ = s.write_all(&[c]);
            }
            None => eprintln!("srvera-emoji is already running"),
        }
        return;
    }
    if cmd == Some(b'q') {
        return;
    }
    // No daemon yet: become it. A leftover socket file means a previous crash.
    let _ = std::fs::remove_file(&path);
    let listener =
        UnixListener::bind(&path).unwrap_or_else(|e| fail(format!("cannot listen on {}: {e}", path.display())));
    let res = app::run(listener, cmd.is_some(), paste_key);
    let _ = std::fs::remove_file(&path);
    if let Err(e) = res {
        fail(e.to_string());
    }
}

fn fail(msg: String) -> ! {
    eprintln!("srvera-emoji: {msg}");
    std::process::exit(1)
}
