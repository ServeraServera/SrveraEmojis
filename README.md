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
   git clone https://github.com/ServeraServera/SrveraEmojis.git
   ```
2. Switch to the crate folder:
   ```bash
   cd SrveraEmojis/main
   ```
3. Install Rust (`cargo`), then build from `main/`:
   ```bash
   cargo build --release
   ```
4. Install the binary:
   ```bash
   install -Dm755 target/release/srvera-emoji ~/.local/bin/srvera-emoji
   ```
5. Let it type the paste key (once, needs root). Many systems already have this rule:
   ```bash
   echo 'KERNEL=="uinput", TAG+="uaccess"' | sudo tee /etc/udev/rules.d/60-srvera-uinput.rules
   ```
   Then reboot. Without it, picks are only copied to the clipboard.
6. Start it at login: add `~/.local/bin/srvera-emoji` in System Settings → Autostart, then
   start it once now with `srvera-emoji &`.
7. Plasma's own Emoji Selector also uses (Super + .). Give one of them another key in
   System Settings -> Shortcuts. The picker prints a warning naming the other shortcut if
   the key is taken.

On other desktops, bind a key to `srvera-emoji --toggle` yourself.

## Features
### RAM usage (≧◡≦)
- With the RAM prices of today, I consider that having a emoji picker gobble up 50-150MBs of RAM is ***preposterous***
- Hopefully this picker only utilizes 3-7MBs idle and 9-15 with the popup open (ﾟﾛﾟ)
<p align="left">
  <img src="docs/ramusage_comparison.png" alt="Comparison" width="600">
</p>
The usage mentioned was for Pss

### No manual copy-pasting
- Click an Emoji or Kaomoji and it's pasted straight into the app you were typing in
- Whatever text you had copied is put back right after, so your clipboard stays intact, *splendid*
- Picked Emojis & Kaomojis don't clutter *Klipper's* clipboard history either

### Doesn't steal your window's focus ( ˘ω˘ )
- The popup opens without taking over your keyboard, so the app you're typing in stays in focus
- Built on Wayland's layer-shell, so it shows up on top of all windows, even fullscreen games and videos

### Over 55k Kaomojis
Yes, you heard that right (◍•ᴗ•◍)♡
<p align="left">
  <img src="docs/55kaomojis.gif" alt="Showcase" width="600">
</p>
