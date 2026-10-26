# SrveraEmojis - Emoji/Kaomoji selector for Wayland (｀・ω・´)

- A ultra-lightweight background Emoji & Kaomoji picker for Wayland ( ˶ˆ꒳ˆ˵ ).
<p align="center">
  <img src="docs/header.gif" alt="Demo" width="600">
</p>
- Press (Super + .) anywhere, pick an Emoji or Kaomoji, and it is pasted right into the app you were typing in (*ﾉωﾉ).
- Built and tested on KDE Plasma 6. DOES NOT WORK ON GNOME

## Installation
### Install from source
1. Clone from source:
   ```bash
   git clone https://github.com/ServeraServera/SrveraEmojis/tree/main
   ```
2. Switch to main:
   ```bash
   cd main
   ```
2. Install Rust (`cargo`), then build from `main/`:
   ```bash
   cargo build --release
   ```
   The Twemoji SVGs go in `main/assets/twemoji/` (from jdecked/twemoji). If that folder is
   empty, download them there first.
3. Install the binary:
   ```bash
   install -Dm755 target/release/srvera-emoji ~/.local/bin/srvera-emoji
   ```
4. Let it type the paste key (once, needs root). Many systems already have this rule:
   ```bash
   echo 'KERNEL=="uinput", TAG+="uaccess"' | sudo tee /etc/udev/rules.d/60-srvera-uinput.rules
   ```
   Then log out and back in. Without it, picks are only copied to the clipboard.
5. Start it at login: add `~/.local/bin/srvera-emoji` in System Settings → Autostart, then
   start it once now with `srvera-emoji &`.
6. Plasma's own Emoji Selector also uses (Super + .). Give one of them another key in
   System Settings -> Shortcuts. The picker prints a warning naming the other shortcut if
   the key is taken.

On other desktops, bind a key to `srvera-emoji --toggle` yourself.
