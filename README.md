<p align="center">
  <img src="main/assets/headericon.svg" alt="Header" width="900">
</p>

# A ultra-lightweight Emoji/Kaomoji picker for Wayland

A Emoji & Kaomoji picker for Wayland built on tiny-skia with no GTK or Qt 
toolkits to stay **as light as possible** ( ˘ω˘ ), built in pure Rust ( ˶ˆ꒳ˆ˵ ).

<p align="left">
  <img src="docs/header.gif" alt="Demo" width="600">
</p>

- Press (Super + .) anywhere, pick an Emoji or Kaomoji, and it is pasted right into the app you were typing in (*ﾉωﾉ).
- Built and tested on KDE Plasma 6. **DOES NOT WORK ON GNOME**

## Features
### No manual copy-pasting Σ(･ω･ﾉ)
- Click an Emoji or Kaomoji and it's pasted straight into the app you were typing in
- Whatever text you had copied is put back right after, so your clipboard stays intact, *splendid*
- Picked Emojis & Kaomojis don't clutter *Klipper's* clipboard history either

### Doesn't steal your window's focus ( ˘ω˘ )
- The popup opens without taking over your keyboard, so the app you're typing in stays in focus
- Built on Wayland's layer-shell, so it shows up on top of all windows, even fullscreen games and videos

### Instantaneous at any time
- Do you still use HDDs in the big 2026? Tired of KDE-Emojier taking several seconds to launch?
- Then today's your lucky day (´・∀・｀)!
- I built Srvera emojis to open up in an instant on an HDD, no matter if you were using a RAM heavy program beforehand or if your disk usage was overloaded (*≧▽≦)
  *cough cough, SWAP MEMORY*

 https://github.com/user-attachments/assets/ddc6d579-e0f9-403f-b3eb-562c7f127acb

*Tested and recorded from a Seagate Barracuda ST2000DM006 7200RPM, 64MB of cache on a CachyOS install*

### RAM usage (≧◡≦)
- With the RAM prices of today, I consider that having a emoji picker gobble up 50-150MBs of RAM is ***preposterous***
- Fortunately this picker only utilizes 3-7MBs idle and 9-15 with the popup open (ﾟﾛﾟ)
<p align="left">
  <img src="docs/ramusage_comparison.png" alt="Comparison" width="600">
</p>

KDE's Emojier is using 139MBs of RAM, that's like 3-4$ of DDR5 RAM (⊙_☉)
*The usage mentioned was for Pss*

### Your favorites tab
Hover your mouse over a Emoji or Kaomoji and then right click it to add it to the favorites tab (⌒▽⌒)

### Over 55k Kaomojis
Yes, you heard that right, so many expressions (◍•ᴗ•◍)♡
Sourced from the wonderful people of kaomojis.jp
<p align="left">
  <img src="docs/55kaomojis.gif" alt="Showcase" width="600">
</p>

## Installation

### Debian, Kubuntu, KDE neon, Mint (.deb)
```bash
sudo apt install ./srvera-emoji_0.20.0-1_amd64.deb
```

### Fedora KDE (.rpm)
```bash
sudo dnf install ./srvera-emoji-0.20.0-1.x86_64.rpm
```

### Any other distro (.tar.gz)
```bash
tar xzf srvera-emoji-0.20.0-x86_64.tar.gz
cd srvera-emoji-0.20.0-x86_64
./install.sh
```
It installs to your home folder and only asks for sudo once, to let the picker type the paste key.
Run `./uninstall.sh` from the same folder to remove it.

### I use arch btw 🤓☝️, CachyOS, EndeavorOS, Manjaro (AUR)
```bash
Coming soon until new account sign ups are enabled (｡•́︿•̀｡)
```

### After installing
- Plasma's own Emoji Selector also uses (Super + .). Give one of them another key in
  System Settings → Shortcuts. The picker's entry there is called "Srvera Emoji".
- On desktops other than KDE, bind a key to `srvera-emoji --toggle` yourself, for example in Sway:
  `bindsym $mod+period exec srvera-emoji --toggle`.

## Building from source
1. Clone the repo:
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

## Architecture
Srvera Emoji is a single Rust binary with no GUI toolkit. Everything it shows is drawn by
hand, and everything it needs is compiled into the binary.

### A background daemon (｡•̀ᴗ-)✧
- Why a background process and not on demand? I still run an HDD for my OS and I wanted the picker
  to feel *snappy* so the tradeoff of 3-7MBs of permanent RAM usage is insignificant (I have 16GBs)
  compared to the benefit of having an instantaneous open even when I am using a lot of swap memory
- `srvera-emoji` starts hidden at login and waits. Opening the picker never starts a new
  process: (Super + .) is registered with KDE's shortcut service (kglobalaccel) over D-Bus,
  and each press arrives as a signal to the running daemon (~25 ms to open, even after
  long idle).

### The D-Bus client
- The shortcut is handled by a tiny D-Bus client (~340 lines in `shortcut.rs`) instead of a
  full D-Bus crate. It speaks just enough of the protocol to log in to the session bus, make
  three calls and read two kinds of signals.
- `srvera-emoji --toggle`, `--show` and `--quit` talk to the daemon through a Unix socket
  in `$XDG_RUNTIME_DIR`. This is how other desktops and scripts open it.
- At startup it registers the action **Srvera Emoji -> Show or hide the picker** with kglobalaccel,
  using Super + . as its default. If you change the key in System Settings -> Shortcuts, your choice is kept.
- Each key press arrives as a `globalShortcutPressed` signal on the already-open bus
  connection, so nothing is started from disk as stated before.
- If KWin restarts, the daemon notices kglobalaccel coming back and registers again.
- If another action already holds the same key, the daemon names it in its log so you know
  what to change.
- It is only a client: it owns no D-Bus name and offers no methods, so other programs have
  nothing to call. This avoids the possibility of malware utilizing the picker as a proxy

### Data built in at compile time
- `build.rs` reads `emoji-test.txt` (Unicode's emoji list), `kaomojis.txt` and the
  Twemoji SVGs, and packs them into three compact tables embedded in the binary.
- Each emoji and kaomoji belongs to exactly one tab, decided at build time: emojis by their
  Unicode group, kaomojis by the first word of their keyword. Opening a tab simply shows its
  pre-sorted slice, and search is a separate keyword lookup.
- Each SVG is compressed on its own and only decompressed when that emoji is on screen.

### A layer-shell popup 
- The window is a Wayland layer-shell overlay, so it stays above fullscreen apps. It opens
  without keyboard focus so the app you're typing in keeps it. Clicking the search box takes
  the keyboard; picking something gives it back.
- It opens right next to the mouse pointer. Wayland doesn't tell apps where the pointer is, so
  the window first covers the screen invisibly for a moment and opens where the pointer enters it.
- KWin can't move layer surfaces, so dragging the title bar changes the window's margins instead.

### Pasting without touching your clipboard
1. The text currently on your clipboard is read and kept aside.
2. The pick is offered as the clipboard (via `ext-data-control`), marked with
   `x-kde-passwordManagerHint: secret` so Klipper doesn't record it. *(The popup "The clipboard currently holds a sensitive piece of data that will not be stored in the history." might appear)*
3. Shift + Insert is sent through a virtual keyboard (`/dev/uinput`), which pastes into the
   focused app, terminals included.
4. Half a second later, your previous clipboard text is put back.

### One udev rule, and only for the paste key
- Srvera Emoji never talks to udev and doesn't install any rules. It only opens
  `/dev/uinput` and creates one virtual keyboard named *srvera-emoji virtual keyboard*.
- That keyboard knows exactly four keys (Ctrl, Shift, V, Insert) and only ever presses
  Shift + Insert or Ctrl + V. It can't read your real keyboard.
- The packages (`.deb`, `.rpm`) and `install.sh` add one udev rule,
  `60-srvera-uinput.rules`, which lets the user sitting at the desktop write to
  `/dev/uinput` (the standard `uaccess` tag). It's the same rule Steam and KDE Connect ship,
  so if you have either, you already have this access.

### Drawing by hand 
- The UI is drawn with *tiny-skia* into a shared-memory buffer: rounded rectangles, text and
  images. Only visible rows are drawn, and a new frame is drawn only when the compositor
  asks for one.
- Emojis are always Twemoji, rendered with resvg at their exact on-screen size and briefly
  cached. This includes emojis inside kaomojis, e.g. `(*´∀｀)ﾉ🎂`.
- Kaomoji text is shaped with rustybuzz and its glyph outlines are filled directly. At
  startup the daemon finds the right installed font for every character used by the 55k
  kaomojis, in one pass, so scrolling never searches through fonts.
- HiDPI and fractional scaling are supported (`wp_fractional_scale` + `wp_viewporter`).

### Staying light 
- Fonts are memory-mapped, not loaded, and everything only needed while the window is open
  (pixel buffers, glyph faces, emoji images) is freed when it closes.
- Favorites and recents are two small text files in `~/.local/share/srvera-emoji/`.

### Source layout
| File | Role |
|---|---|
| `main/build.rs` | Packs the emoji/kaomoji lists and SVGs into the binary |
| `main/src/main.rs` | Command line and single-instance daemon |
| `main/src/app.rs` | Wayland: window, input, scaling, frame timing, event loop |
| `main/src/ui.rs` | Layout, drawing and interaction (knows nothing about Wayland) |
| `main/src/data.rs` | Access to the embedded tables and keyword search |
| `main/src/text.rs` | Font fallback, shaping and glyph drawing |
| `main/src/emoji_img.rs` | Twemoji SVG decoding and rendering |
| `main/src/paste.rs` | Clipboard handling and the virtual keyboard |
| `main/src/shortcut.rs` | Super+. through KDE's kglobalaccel (minimal D-Bus client) |

## AI Disclosure
### How it was used
The architecture was made by me then all of the code was generated by an LLM and 
reviewed by me before committing any changes. 

### Some personal background
- I recently switched to Linux for good instead of keeping a small dualboot partition for stuff
  that Windows couldn't do and I really did miss Window's Kaomoji tab on their picker.
- I didn't have much time to spare and I really didn't feel like coding it by hand.
  I do not take credit for the code, It's the LLMs work, but for something I was going to use everyday,
  I had to do the bare minimum of at least taking my time to review the code and not make it sloppy
