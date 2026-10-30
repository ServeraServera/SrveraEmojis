# Changelog


## [0.20] - 2026-10-30
Commit: `380a1f6`

### Fixed
- The paste key is no longer pressed if another program replaced the clipboard or the
  primary selection between the pick and the paste (70 ms). Before, that program's text
  could be pasted instead, e.g. a command into a terminal.
- Simplified the icon to avoid rendering issues making it look weird on smaller sizes

## [0.19] - 2026-10-28
Commit: `9251125`

### Fixed
- The search box caret now only shows while the search box has the keyboard, and blinks
  (it used to be a static bar all the time).
- Copying something within half a second of a pick no longer gets replaced by the old
  clipboard text.
- Text highlighted before a pick (middle-click paste) is now put back, instead of cleared.
- Two picks less than 70 ms apart no longer paste the second one twice.
- Search: keywords starting with the first query word rank first for multi-word queries too
  ("red heart" → ❤️).
- `--paste-key` sent to an already running picker now prints a warning instead of being ignored.
- An emoji that fails to render no longer empties the image cache on every frame.

### Changed
- The window opens next to the mouse pointer every time (its top-left corner at the pointer,
  kept on screen) instead of centered. A dragged position is no longer remembered
  after closing.
- Emojis inside kaomoji text are found with one lookup for most characters (Seasonal tab
  draws in ~21 ms instead of ~26 ms; frames unchanged).
- Added new program Icon

## [0.18] - 2026-10-26
Commit: `c3b4275`

### Changed
- Super+. now reaches the running picker as a D-Bus signal from KDE's shortcut service
  instead of starting a new `srvera-emoji --toggle` process on every press. After a long
  idle or heavy RAM use, opening went from ~500 ms (re-reading the program from disk)
  to ~25 ms; warm opens from ~40 ms to ~15 ms.
- The shortcut is registered by the picker itself (default Meta+.) and can be changed in
  System Settings → Shortcuts → "Srvera Emoji"; the .desktop entry no longer carries one.

### Added
- A warning on start when another shortcut already holds the key, naming it.
- README: install-from-source steps.

## [0.17] - 2026-10-26
Commit: `eef1f21`

### Changed
- Whole crate formatted with rustfmt (`main/rustfmt.toml`: 120 columns, `use_small_heuristics = "Max"`).
- Code condensed from 2,621 to 2,291 lines with pixel-identical rendering: color constants,
  one emoji-drawing helper, UI drawing in logical units, shorter protocol handler signatures,
  table-driven CLI flags.
- AGENTS.md: new "Formatting" rules; the slow open after a long idle is diagnosed (each
  Super+. starts a new process that must be re-read from the HDD).

## [0.16] - 2026-10-25
Commit: `695e92a`

### Added
- Emojis inside kaomojis (eg; `(*´∀｀)ﾉ🎂`) are drawn with the Twemoji images instead of
  utilizing system fonts.

### Fixed
- Kaomoji scrolling stutter and 20–30 MB memory spikes: fonts for every kaomoji character
  are now found in one pass at startup instead of scanning all installed fonts per character (◕ᴗ◕).
- Font files were unmapped before their parsed face was dropped.

### Changed
- Docs brought up to date (file locations, kaomoji tab rules, known issues).

## [0.15] - 2026-10-25
Commits: `2ed6303`, `b803d59`

### Changed
- Renamed from placeholder name to Srvera Emoji: binary `srvera-emoji`, socket
  `srvera-emoji.sock`, data folder `~/.local/share/srvera-emoji/`, window title.

### Added
- Custom app icon (`main/assets/icon.svg`).

## [0.14] - 2026-10-25
Commits: `d0dbd49` … `a8285fa`

### Changed
- The kaomoji list (`kaomojis-msime.txt`) and the Unicode emoji list (`emoji-test.txt`) now
  live inside `main/`, and the build reads them from there.

### Added
- AGENTS.md (project context for coding agents) and CLAUDE.md.

### Removed
- The kaomoji categorizing Python script.

## [0.13] - 2026-10-24
Commits: `ef83c08` (plus README/license updates and a duplicate folder removal)

### Added
- Kaomoji category names in the sidebar carry a matching kaomoji, e.g. "Happy (◠‿◠)".

### Changed
- The "People" emoji tab is now "People/Body".

## [0.12] - 2026-10-24
Commit: `aba66a7`

### Added
- Favorites: right-click an item to add or remove it; favorites get a small ⭐.
- "Clear" button on the Recent tab.
- The clipboard's previous text is put back after a pick is pasted.
- Picks are marked as secret so Klipper's clipboard history skips them.

## [0.11] - 2026-10-24
Commit: `7180525`

### Added
- Move the window by dragging its title bar; close button.
- Interactable scrollbar: drag the thumb or click the track.

### Changed
- The window opens without taking the keyboard, so the app you're typing in keeps focus.
  Click the search box to type there.
- The crate moved from `test/` to `main/`.

### Fixed
- Memory could balloon to 400+ MB (;°△°): drawing now happens only on compositor frame callbacks,
  not on every input event.

## [0.10] - 2026-10-24
Commits: `e2ad4ba`, `20873c1`, `7156a2a`

### Added
- First working build: a background daemon for Wayland (KDE Plasma 6) that opens on Super+.
  as a layer-shell popup.
- About 3,950 Twemoji emojis and 55k kaomojis, grouped in category tabs, with an
  Emoji/Kaomoji switch.
- Keyword search across both (skin-tone variants only in search) and a Recent tab.
- Picks are pasted into the focused app through the clipboard and a virtual keyboard.
- Fractional scaling and memory freed when the window hides.
