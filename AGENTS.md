# SrveraEmojis — AGENTS.md

## Goal
An ultra-lightweight emoji & kaomoji picker for Wayland (built and tested on KDE Plasma 6).
It runs in the background, opens on Super+., searches/browses ~3,950 emojis and ~55k
kaomojis, and pastes the pick straight into the focused app.
Targets: ≤15 MB RAM with the window open (currently ~11 MB, ~8 MB idle), a few MB idle, instant open.

## Coding conventions (always follow)
- **Least code possible, lightest program possible.** Prefer reusing existing helpers and
  editing data files over new modules, abstractions or crates. No new dependency without a clear reason.
- Measure memory after changes: `awk '/^Rss|^Pss:/' /proc/$(pgrep -x srvera-emoji)/smaps_rollup`
  (window open and closed).
- No GUI toolkit: everything is drawn by hand with tiny-skia. Keep it that way.
- Crate versions are pinned to known APIs (SCTK 0.19, resvg 0.45 with its bundled
  `resvg::tiny_skia`, rustybuzz 0.20, fontdb 0.23). Don't bump them casually.
- Comments are short and plain, explaining why, not what.
- Draw only on compositor frame callbacks (`frame_pending`/`dirty` in app.rs): drawing
  per input event once made memory balloon to 400+ MB.
- Free everything window-only on hide (`Ui::release`, `malloc_trim`).

## Formatting (always follow)
- rustfmt with `main/rustfmt.toml`: `max_width = 120`, `use_small_heuristics = "Max"` (short
  `if`/`else`, struct literals and calls stay on one line when they fit). Run `cargo fmt` after
  every change; `cargo fmt --check` and `cargo clippy --release` must be clean. Never hand-format
  around rustfmt or add `#[rustfmt::skip]`.
- Condense with code, not with formatting: helpers over repeated blocks (`Fonts::emoji`,
  `fill_rr`, `Row::cell_w`, `Action::redraw_if`), table-driven code (CLI flags in main.rs),
  `..Default::default()` for structs with many empty fields, tuple assignment for related state.
- Short names for long protocol types so handler signatures fit on one line: `Qh` =
  `QueueHandle<App>` (app.rs), and the `WlSurface`/`WlKeyboard`/`WlOutput`/… imports.
  Handlers with no interesting events use `ignore_events!` (defined in main.rs).
- UI code works in logical px (scale 1). The scale is applied in one place: `Fonts::scale`
  (set at the start of `Ui::draw`) for text and emoji images, the `s` argument of `fill_rr`
  for shapes, and the clip mask built in `Ui::draw`. No `* s` / `/ s` anywhere else.
- Colors are RGBA `u32` constants (`0x3DAEE9FF`, palette at the top of ui.rs) turned into a
  paint by `text::paint`.
- Long string lists are one comma-separated string (`PREFERRED` in text.rs) rather than an
  array with one item per line.
- Large files are split with `// ---------------------------------------------------------------- name`
  dividers. Doc comments (`///`, `//!`) on every module and on non-obvious functions.

## Layout
- `kaomojis-msime.orig.txt` — untouched original (MS IME dictionary format); kept locally, not in git.
- `main/` — the Rust crate (binary `srvera-emoji`):
  - `rustfmt.toml` — formatting rules (see Formatting).
  - `emoji-test.txt` — Unicode emoji list (source of emojis, groups, names).
  - `kaomojis-msime.txt` — kaomojis as `keyword<TAB>kaomoji`; the keyword's first word is its tab.
  - `build.rs` — parses both .txt files + `assets/twemoji/*.svg` into compact blobs embedded in the binary.
  - `src/main.rs` — CLI and single-instance daemon via `$XDG_RUNTIME_DIR/srvera-emoji.sock`.
  - `src/app.rs` — Wayland side: layer-shell popup, shm buffers, input, scaling, focus, placing at the
    pointer, moving, caret blink, event loop.
  - `src/ui.rs` — state, layout, drawing, hit-testing, favorites/recent lists. Knows nothing about Wayland.
  - `src/data.rs` — read-only access to the embedded tables + keyword search.
  - `src/text.rs` — kaomoji/UI text: fontdb fallback, rustybuzz shaping, glyph outlines → tiny-skia;
    emojis inside text are drawn with the Twemoji SVGs.
  - `src/emoji_img.rs` — inflate an emoji's SVG, render with resvg, small cache.
  - `src/paste.rs` — clipboard (ext-data-control) + virtual keyboard (uinput).
  - `src/shortcut.rs` — Super+. through KDE's kglobalaccel: a minimal D-Bus client (no crate).
  - `assets/twemoji/` — Twemoji 17 SVGs (from jdecked/twemoji).
  - `assets/icon.svg` — app icon, installed as `~/.local/share/icons/hicolor/scalable/apps/srvera-emoji.svg`
    (used by the .desktop entries).
  - `packaging/` — release files: uinput udev rule, menu + autostart .desktop entries, `postinst`
    (deb/rpm: applies the udev rule), `install.sh`/`uninstall.sh` (tarball, per-user).
- `binaries/` — built release packages (.deb, .rpm, `tarfile/*.tar.gz`).

## How it works
1. **Build time:** `build.rs` assigns every item to exactly one tab and stores items grouped
   by tab, plus a keyword table, per-emoji deflated SVGs and emoji ids sorted by text (to spot
   emojis inside text). Emojis: tab = `# group:` in emoji-test.txt (short names in `EMOJI_CATS`).
   Kaomojis: tab = first word of the keyword, matched against the first part of each `KAO_CATS`
   pair `(word, label)`; the second part is only the sidebar label. If the first word matches
   nothing, any keyword word (plurals too) is tried, then "Other". Skin-tone variants show only in search.
2. **Daemon:** `srvera-emoji` starts hidden and picks a font for every kaomoji character once
   (one pass over the installed fonts, 0.25–1 s while hidden), so scrolling never scans fonts.
   Super+. reaches it as a D-Bus signal: at start it registers the action `srvera-emoji/toggle`
   with kglobalaccel (inside KWin), default key Meta+., and listens for `globalShortcutPressed`.
   No process is started per press (that used to cost ~0.5 s from the HDD after memory pressure;
   now ~25 ms cold). It re-registers when kglobalaccel restarts, and warns on stderr if another
   shortcut holds the key. `--toggle`/`--show`/`--quit` still talk to it over the Unix socket
   (scripts, other desktops, app menu); `--toggle` becomes the daemon if none runs.
   `--paste-key ctrl-v` changes the paste key.
3. **Window:** a wlr-layer-shell overlay (KWin can't move layer surfaces, so moving = changing
   top/left margins, exclusive zone -1 so they count from the output corner). Opening at the
   pointer: the surface first covers the output with a 1×1 transparent buffer stretched by the
   viewport (`cover`), and the pointer's enter position places it (`place`: top-left corner at
   the pointer, clamped on screen); 150 ms without an enter → centered. The position is
   kept in `Window`, so it's forgotten on hide. It opens **without** keyboard focus so the user's
   app keeps it; clicking the search box grabs the keyboard (Exclusive) and starts the caret
   blink timer (`BLINK`), any pick gives it back.
4. **Tabs:** Favorites (right click toggles, ⭐ marks), Recent (with Clear), then categories;
   separate per Emoji/Kaomoji mode. Lists are saved in `~/.local/share/srvera-emoji/`.
5. **Paste:** read and save the current clipboard and primary selection text → offer the pick as
   both, marked `x-kde-passwordManagerHint: secret` so Klipper skips it → 70 ms later
   Shift+Insert through a uinput keyboard (only the latest pick pastes) → 500 ms later restore
   each saved text, unless something else took that one meanwhile.
6. **Rendering:** full redraw into a shm buffer (ABGR8888 when available), only visible rows,
   content clipped with a mask; fractional scaling via wp_fractional_scale + viewporter.
   Emojis are always Twemoji SVGs, also inside kaomojis and text (only fully-qualified emojis
   count, so text symbols like ♡ or a bare ☀ stay font glyphs).

## Build, test, install
Run from `main/`:
- Tests: `cargo test --release`; also `cargo fmt --check` and `cargo clippy --release`
- UI snapshots: `SNAP_DIR=<dir> cargo test --release snapshots -- --ignored`
  (writes raw RGBA 600×496 frames; convert with `magick -size 600x496 -depth 8 rgba:f.rgba f.png`)
- Install + restart (after any change to code, build.rs or the .txt files):
  `cargo build --release && install -m755 target/release/srvera-emoji ~/.local/bin/ && srvera-emoji --quit`
  (then start it again: `srvera-emoji &`, or log out and in for autostart).
- Release packages (needs zig, cargo-zigbuild, cargo-deb, cargo-generate-rpm): build for glibc 2.31 with
  `RUSTFLAGS="-C link-arg=-Wl,--allow-shlib-undefined" cargo zigbuild --release --target x86_64-unknown-linux-gnu.2.31`
  (the flag is needed when the host's libxkbcommon was built against a newer glibc, as on Arch: it
  references `stat64@GLIBC_2.33` etc., which the 2.31 stubs lack; at run time the user's own
  libxkbcommon is loaded, so this is safe), then
  `cargo deb --no-build --no-strip --target x86_64-unknown-linux-gnu -o ../binaries/` and
  `cargo generate-rpm --target x86_64-unknown-linux-gnu -o ../binaries/` (config in Cargo.toml). The tarball is
  that binary + `packaging/` files + `assets/icon.svg` (as `srvera-emoji.svg`) + both licenses, in
  `binaries/tarfile/srvera-emoji-<version>-x86_64.tar.gz`.
- System setup (already done): autostart entry in `~/.config/autostart/` (runs the daemon),
  `/dev/uinput` writable by the user (udev `uaccess`), Plasma's own emoji selector moved off
  Meta+. (to Meta+Ctrl+Alt+Shift+Space). The shortcut lives in System Settings → Shortcuts →
  "Srvera Emoji"; `srvera-emoji.desktop` has no shortcut any more (app menu only).

## Editing categories (the user does this)
- Move an emoji: move its line(s) under another `# group:` in main/emoji-test.txt.
- Move a kaomoji: change the first word of its keyword in main/kaomojis-msime.txt.
- Add, rename or reorder kaomoji tabs: edit `KAO_CATS` in build.rs and make the first keyword
  words in main/kaomojis-msime.txt match.

## Known issues / next up
- Super+. only works while the daemon runs (autostart starts it); there is no process to launch.
- The global shortcut needs KDE (kglobalaccel). Elsewhere, bind `srvera-emoji --toggle` yourself.
- Characters typed in the search box that no kaomoji uses still cost one font scan each.
- Only text clipboard contents can be restored after a paste; images/files are cleared.
- 19 Unicode 18 emojis have no Twemoji SVG yet and are skipped.

## Desktop support (for later)
Requirements: **layer-shell** is hard (`LayerShell::bind` in app.rs; the daemon exits without it).
**ext-data-control** is soft (without it the picker opens but can't copy/paste); only the ext
version is used, not the older `zwlr_data_control_manager_v1`. uinput, fonts and scaling don't
depend on the desktop.

| Desktop | Opens | Pastes |
|---|---|---|
| KDE Plasma 6 | ✅ | ✅ (tested) |
| Sway, labwc, Wayfire, river | ✅ | ✅ with wlroots 0.19+, older versions only have wlr-data-control |
| Hyprland, niri, COSMIC | ✅ | Likely on current versions (unverified which release added ext-data-control) |
| GNOME, Weston, X11 | ❌ | ❌ (no layer-shell / not Wayland) |

labwc also covers LXQt, Budgie 10.10 and some Xfce Wayland setups. Outside KDE the shortcut must
be bound to `srvera-emoji --toggle` by hand (Sway: `bindsym $mod+period exec srvera-emoji --toggle`,
Hyprland: `bind = SUPER, period, exec, srvera-emoji --toggle`).

Todo, in order of value:
1. **wlr-data-control fallback** in paste.rs (~30–50 lines): bind `zwlr_data_control_manager_v1`
   when ext isn't there. Same requests and events, different names; `wayland-protocols-wlr` is
   already in the dependency tree via SCTK. Makes every "Opens ✅" row paste too.
2. **Regular-window fallback** for GNOME (~150–250 lines): an xdg-shell toplevel when layer-shell
   is missing. Needs the `XDG_ACTIVATION_TOKEN` from the shortcut forwarded over the socket by
   `--toggle` (else focus-stealing prevention may open it unfocused), and close on focus loss.
   Can't force position or stay-on-top there. GNOME also has no ext/wlr-data-control, so pasting
   would need another route.
