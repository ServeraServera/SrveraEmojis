# SrveraEmojis — AGENTS.md

## Goal
An ultra-lightweight emoji & kaomoji picker for Wayland (built and tested on KDE Plasma 6).
It runs in the background, opens on Super+., searches/browses ~3,950 emojis and ~55k
kaomojis, and pastes the pick straight into the focused app.
Targets: ≤15 MB RAM with the window open (currently ~9–10 MB), a few MB idle, instant open.

## Coding conventions (always follow)
- **Least code possible, lightest program possible.** Prefer reusing existing helpers and
  editing data files over new modules, abstractions or crates. No new dependency without a clear reason.
- Measure memory after changes: `awk '/^Rss|^Pss:/' /proc/$(pgrep -x emoji-picker)/smaps_rollup`
  (window open and closed).
- No GUI toolkit: everything is drawn by hand with tiny-skia. Keep it that way.
- Crate versions are pinned to known APIs (SCTK 0.19, resvg 0.45 with its bundled
  `resvg::tiny_skia`, rustybuzz 0.20, fontdb 0.23). Don't bump them casually.
- Comments are short and plain, explaining why, not what.
- Draw only on compositor frame callbacks (`frame_pending`/`dirty` in app.rs): drawing
  per input event once made memory balloon to 400+ MB.
- Free everything window-only on hide (`Ui::release`, `malloc_trim`).

## Layout
- `emoji-test.txt` — Unicode emoji list (source of emojis, groups, names).
- `kaomojis-msime.orig.txt` — untouched original (MS IME dictionary format); not tracked in git.
- `main/` — the Rust crate (binary `emoji-picker`):
  - `kaomojis-msime.txt` — kaomojis as `keyword<TAB>kaomoji`; the keyword's first word is its tab.
  - `build.rs` — parses both .txt files + `assets/twemoji/*.svg` into compact blobs embedded in the binary.
  - `src/main.rs` — CLI and single-instance daemon via `$XDG_RUNTIME_DIR/emoji-picker.sock`.
  - `src/app.rs` — Wayland side: layer-shell popup, shm buffers, input, scaling, focus, moving, event loop.
  - `src/ui.rs` — state, layout, drawing, hit-testing, favorites/recent lists. Knows nothing about Wayland.
  - `src/data.rs` — read-only access to the embedded tables + keyword search.
  - `src/text.rs` — kaomoji/UI text: fontdb fallback, rustybuzz shaping, glyph outlines → tiny-skia.
  - `src/emoji_img.rs` — inflate an emoji's SVG, render with resvg, small cache.
  - `src/paste.rs` — clipboard (ext-data-control) + virtual keyboard (uinput).
  - `tools/categorize_kaomoji.py` — rewrites the kaomoji file, prefixing keywords with their category.
  - `assets/twemoji/` — Twemoji 17 SVGs (gitignored, downloaded from jdecked/twemoji).

## How it works
1. **Build time:** `build.rs` assigns every item to exactly one tab and stores items grouped
   by tab, plus a keyword table and per-emoji deflated SVGs. Emojis: tab = `# group:` in
   emoji-test.txt (short names in `EMOJI_CATS`). Kaomojis: tab = first word of the keyword,
   matched against the first part of each `KAO_CATS` pair `(word, label)`; the second part is
   only the sidebar label. Unmatched → "Other". Skin-tone variants show only in search.
2. **Daemon:** `emoji-picker` starts hidden. `--toggle`/`--show`/`--quit` talk to it over the
   socket; `--toggle` becomes the daemon if none runs. `--paste-key ctrl-v` changes the paste key.
3. **Window:** a wlr-layer-shell overlay (KWin can't move layer surfaces, so moving = changing
   top/left margins). It opens **without** keyboard focus so the user's app keeps it; clicking the
   search box grabs the keyboard (Exclusive), any pick gives it back.
4. **Tabs:** Favorites (right click toggles, ⭐ marks), Recent (with Clear), then categories;
   separate per Emoji/Kaomoji mode. Lists are saved in `~/.local/share/emoji-picker/`.
5. **Paste:** read and save the current clipboard text → offer the pick as clipboard + primary
   selection, marked `x-kde-passwordManagerHint: secret` so Klipper skips it → 70 ms later
   Shift+Insert through a uinput keyboard → 500 ms later restore the saved text.
6. **Rendering:** full redraw into a shm buffer (ABGR8888 when available), only visible rows,
   content clipped with a mask; fractional scaling via wp_fractional_scale + viewporter.

## Build, test, install
Run from `main/`:
- Tests: `cargo test --release`
- UI snapshots: `SNAP_DIR=<dir> cargo test --release snapshots -- --ignored`
  (writes raw RGBA 600×496 frames; convert with `magick -size 600x496 -depth 8 rgba:f.rgba f.png`)
- Install + restart (after any change to code, build.rs or the .txt files):
  `cargo build --release && install -m755 target/release/emoji-picker ~/.local/bin/ && emoji-picker --quit`
  (the next Super+. starts the new version).
- System setup (already done): Super+. → `emoji-picker.desktop` in kglobalshortcutsrc
  (Plasma's own emoji selector moved to Meta+Ctrl+Alt+Shift+Space), autostart entry in
  `~/.config/autostart/`, `/dev/uinput` writable by the user.

## Editing categories (the user does this)
- Move an emoji: move its line(s) under another `# group:` in emoji-test.txt.
- Move a kaomoji: change the first word of its keyword in main/kaomojis-msime.txt.
- Re-categorize all kaomojis: edit `CATS` in tools/categorize_kaomoji.py and rerun it; keep
  `KAO_CATS` in build.rs in the same order with matching words.

## Known issues / next up
- Kaomoji scrolling can stutter and briefly raise RSS to 20–30 MB: characters missing from
  the preferred fonts trigger a scan of every installed font (`Fonts::face_for` slow path).
- Only text clipboard contents can be restored after a paste; images/files are cleared.
- 19 Unicode 18 emojis have no Twemoji SVG yet and are skipped.
