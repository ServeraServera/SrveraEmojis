//! srvera-emoji: a tiny background emoji & kaomoji picker for Wayland.
//!
//!   srvera-emoji            start the background daemon (hidden)
//!   srvera-emoji --toggle   show/hide the picker (starts the daemon if needed)
//!   srvera-emoji --quit     stop the daemon
//!   --paste-key ctrl-v      paste with Ctrl+V instead of Shift+Insert

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
    let cmd = if has("--toggle") {
        Some(b't')
    } else if has("--show") {
        Some(b's')
    } else if has("--quit") {
        Some(b'q')
    } else {
        None
    };

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
    let listener = match UnixListener::bind(&path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("srvera-emoji: cannot listen on {}: {e}", path.display());
            std::process::exit(1);
        }
    };
    let res = app::run(listener, cmd.is_some(), paste_key);
    let _ = std::fs::remove_file(&path);
    if let Err(e) = res {
        eprintln!("srvera-emoji: {e}");
        std::process::exit(1);
    }
}
