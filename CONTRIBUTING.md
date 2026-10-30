# Contributing to SrveraEmojis (｀・ω・´)ゞ

Thanks for wanting to help. Srvera Emoji has one goal above everything else: being THE
***lightest*** Emoji & Kaomoji picker on Wayland, *or atleast that is what I'm trying to ¯\_( ͡❛ ͜ʖ ͡❛)_/¯*.
Every contribution is measured against that.

## Ground rules
- **Least code, lightest program.** A change that adds features but also adds megabytes of
  RAM, a large dependency or a lot of code will most likely be declined. Prefer reusing the
  existing helpers and editing data files over new modules or abstractions.
- **Memory budget:** <15 MB with the popup open, a few MB idle (measured as Pss). Please
  include before/after numbers for any change that touches drawing, fonts or data.
- **No GUI toolkit.** Everything is drawn by hand with tiny-skia, and that stays that way.
  *I don't want some RAM hungry GUI toolkit consuming 10 to 50 srvera emojis*
- **No new crates** without a clear reason. Versions are pinned to known APIs (SCTK 0.19,
  resvg 0.45, rustybuzz 0.20, fontdb 0.23), so please don't bump them as part of another change.

## Easy ways to help (◕ᴗ◕)
No Rust needed for these:
- **Test on another desktop:** Sway, Hyprland, Niri, Labwc, COSMIC, whatever you use.
  Tell us whether the picker opens and whether pasting works.
- **Fix or add a kaomoji:** edit `main/kaomojis-msime.txt`. Each line is
  `keyword<TAB>kaomoji`, and the first word of the keyword decides its tab
  (`crying sobbing	(╥﹏╥)` goes to Crying). Unknown first words go to "Other".
- **Add Emoji keywords:** emojis live in `main/emoji-test.txt`, one per line:
  `1F600 ; fully-qualified # 😀 E1.0 grinning face`. The name after the version is what
  search matches (along with the `# subgroup:` above it), so a better name means better search.
  Only `fully-qualified` lines are used. A new emoji also needs its Twemoji SVG in
  `main/assets/twemoji/`, named after its code points in lowercase (`1f600.svg`); without
  one it's skipped when building.
- **Giving constructive feedback** is highly appreciated, I want to hear from y'all (´,,•ω•,,`)♡
- **Reporting bugs** (see below).

## Reporting bugs
Please include:
- Your distro, DE and version (e.g. KDE Plasma 6.8, Sway 1.11)
- How you installed it (.deb, .rpm, tarball, from source)
- What you did, what you expected and what happened (duh)
- The output of running `srvera-emoji` from a terminal (quit the running one first with
  `srvera-emoji --quit`)

## Building and checking
From `main/`:
```bash
cargo build --release
cargo test --release
cargo fmt --check
cargo clippy --release
```
All four must pass before a pull request. Formatting follows `main/rustfmt.toml`; run
`cargo fmt` and don't hand-format around it.

To try your build:
```bash
install -m755 target/release/srvera-emoji ~/.local/bin/ && srvera-emoji --quit; srvera-emoji &
```

To check how the UI looks without opening the window:
```bash
SNAP_DIR=/tmp cargo test --release snapshots -- --ignored
```
This writes raw 600×496 RGBA frames; convert them with
`magick -size 600x496 -depth 8 rgba:file.rgba file.png`.

### How to measure memory usage
With the picker running, open and closed:
```bash
awk '/^Rss|^Pss:/' /proc/$(pgrep -x srvera-emoji)/smaps_rollup
```

## Code style
- Formatting is done by rustfmt with `main/rustfmt.toml` (120 columns,
  `use_small_heuristics = "Max"`). Run `cargo fmt` after every change; `cargo fmt --check`
  must be clean. Don't hand-format around it or use `#[rustfmt::skip]`.
- Short, plain comments that explain *why*, not *what*. Doc comments on every module and on
  non-obvious functions.
- UI code works in logical pixels; the scale is applied only in `Fonts::scale`, `fill_rr`
  and the clip mask.
- Draw only on compositor frame callbacks. Drawing on every input event once made memory
  grow past 400 MB (ﾟﾛﾟ), crazy right?.
- Free everything the popup needs when it closes (`Ui::release`).

## Language Support
Even though I'm Spanish, I 
- If you want to add language support for the

## Pull requests
- One topic per pull request, keep it small.
- Describe what changed and why. Include memory numbers if relevant *(measure it with the terminal)*, and a screenshot for
  visual changes.
- Add an entry to [CHANGELOG.md](CHANGELOG.md) under a new or the latest version.

## Known Issues
- I plan for a wlr-data-control fallback in `paste.rs`, so pasting works on compositors that only
  have the older clipboard protocol.
- Desktops without layer-shell such as GNOME won't work, even if I implemented a regular window fallback for it,
  the focus functionality of `app.rs` would be lost.
- Super + . only works while the daemon runs (autostart starts it), there is no fallback implemented for it yet as
  the shortcut only talks to the daemon over D-Bus instead of cold starting a new process.
- The global shortcut needs KDE's kglobalaccel. Elsewhere, you will have to bind srvera-emoji --toggle yourself.
- Characters typed in the search box that no kaomoji uses still cost one font scan each.
- Only text clipboard contents can be restored after a paste so images and files are cleared from it.
- 19 Unicode 18 emojis have no Twemoji SVG yet and are skipped for now. I've used SVGs to avoid font displaying issues
  that might come with *cough cough, rolling release updates*
- The program hides picks from the clipboard by marking the Emojis or Kaomojis as `x-kde-passwordManagerHint: secret`, while the Emoji or Kaomoji is still copied,
  the following popup will appear if you view Klipper's history: `The clipboard currently holds a sensitive piece of data that will not be stored in the history.`
- Memory usage can balloon up to 20-35MBs of usage with the popup open, IF... You are a **maniac** and you start scrolling down through ALL, and I mean ALL of the
  tens of thousands of Kaomojis available, but for any normal person, I don't really consider it an issue per se as the memory is freed once the popup is closed

## Notes
Thank you for reading it all through! As stated before, any feedback is appreciated! For now, I just wanted to state that 
don't plan to develop a GNOME implementation any time soon, cheers!

## License
By contributing, you agree that your contributions are licensed under the
[MIT License](LICENSE). The Twemoji graphics in `main/assets/twemoji/` are © Twitter/X and
contributors, licensed under [CC-BY 4.0](main/assets/LICENSE-GRAPHICS).
