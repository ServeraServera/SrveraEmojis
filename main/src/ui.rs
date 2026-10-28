//! Picker state, layout, drawing and input handling. Knows nothing about Wayland:
//! it draws into a pixel buffer and turns key/pointer input into `Action`s.

use crate::{data, text::paint, text::Fonts};
use resvg::tiny_skia::{Color, FillRule, Mask, PathBuilder, PixmapMut, Rect, Transform};
use smithay_client_toolkit::seat::keyboard::{Keysym, Modifiers};

// Logical layout (px at scale 1).
pub const WIDTH: u32 = 600;
pub const HEIGHT: u32 = 496;
const PAD: f32 = 10.0;
const TITLE_H: f32 = 26.0; // drag handle with the close button
const CLOSE_W: f32 = 28.0;
const SEARCH_H: f32 = 36.0;
const SIDEBAR_W: f32 = 150.0;
const TOP: f32 = TITLE_H + SEARCH_H + 10.0; // where sidebar and content start
const MODE_H: f32 = 28.0;
const CAT_Y: f32 = TOP + MODE_H + 8.0;
const CAT_H: f32 = 22.0;
const CX0: f32 = SIDEBAR_W + PAD; // content area
const CX1: f32 = WIDTH as f32 - PAD;
const CY1: f32 = HEIGHT as f32 - PAD;
const EMOJI_CELL: f32 = 44.0;
const EMOJI_SIZE: f32 = 30.0;
const KAO_ROW: f32 = 34.0;
const KAO_COLS: usize = 2;
const UI_FONT: f32 = 13.0;
const KAO_FONT: f32 = 15.0;
const MAX_RECENT: usize = 32;
const RECENT_ICON: &str = "🕘";
const STAR: &str = "⭐";
const CLEAR_W: f32 = 44.0; // "Clear" hit area at the right of the Recent row

// Breeze Dark-like palette, as RGBA.
const WINDOW: u32 = 0x202326FF;
const FIELD: u32 = 0x141618FF;
const BORDER: u32 = 0x3A3F44FF;
const TEXT: u32 = 0xFCFCFCFF;
const DIM: u32 = 0x9AA1A8FF;
const ACCENT: u32 = 0x3DAEE9FF;
const SEL: u32 = 0x3DAEE960;
const HOVER: u32 = 0xFFFFFF16;
const CLOSE: u32 = 0xDA4453FF;

pub enum Action {
    None,
    Redraw,
    Close,
    Pick(u32),
    StartMove,
    FocusSearch,
}

impl Action {
    fn redraw_if(changed: bool) -> Self {
        if changed {
            Action::Redraw
        } else {
            Action::None
        }
    }
}

struct Row {
    y: f32,
    h: f32,
    start: usize,
    len: usize,
    kao: bool,
}

impl Row {
    fn cell_w(&self) -> f32 {
        if self.kao {
            (CX1 - CX0) / KAO_COLS as f32
        } else {
            EMOJI_CELL
        }
    }
}

#[derive(Default)]
pub struct Ui {
    pub query: String,
    pub caret: bool, // search caret, blinked by app.rs while the search box has the keyboard
    kao: bool,
    cat: usize, // 0 = Favorites, 1 = Recent, n = category n-2
    view: Vec<u32>,
    rows: Vec<Row>,
    content_h: f32,
    sel: usize,
    hover: Option<usize>,
    scroll: f32,
    recent: Vec<u32>,
    favs: Vec<u32>,
    fonts: Option<Fonts>,
    mask: Option<Mask>,
    recent_icon: Option<u32>,
    star: Option<u32>,
    close_hover: bool,
    bar_hover: bool,
    bar_grab: Option<f32>, // pointer offset inside the scrollbar thumb while dragging
}

impl Ui {
    pub fn new() -> Self {
        let mut ui = Ui {
            recent: load_list("recent"),
            favs: load_list("favorites"),
            recent_icon: data::find_text(RECENT_ICON),
            star: data::find_text(STAR),
            ..Default::default()
        };
        ui.cat = ui.default_cat();
        ui
    }

    /// Loads the font list and picks a font for every kaomoji char ahead of the first
    /// open, so the window appears instantly and scrolling never scans fonts.
    pub fn warm_up(&mut self) {
        let f = self.fonts.get_or_insert_with(Fonts::new);
        f.prefetch(data::KAO_CHARS.chars());
        f.release();
    }

    /// Called every time the window opens.
    pub fn reset(&mut self) {
        self.query.clear();
        (self.hover, self.close_hover, self.bar_hover, self.bar_grab, self.caret) = (None, false, false, None, false);
        self.rebuild();
    }

    /// Frees everything that is only needed while the window is visible.
    pub fn release(&mut self) {
        if let Some(f) = &mut self.fonts {
            f.release();
        }
        self.mask = None;
        self.view = Vec::new();
        self.rows = Vec::new();
    }

    /// Items of a saved list that belong to the current mode.
    fn mine(&self, list: &[u32]) -> Vec<u32> {
        list.iter().copied().filter(|&i| data::is_emoji(i) != self.kao).collect()
    }

    /// Favorites if this mode has any, else Recent if it has any, else the first category.
    fn default_cat(&self) -> usize {
        if !self.mine(&self.favs).is_empty() {
            0
        } else if !self.mine(&self.recent).is_empty() {
            1
        } else {
            2
        }
    }

    fn cat_count(&self) -> usize {
        2 + if self.kao { data::kao_cat_count() } else { data::emoji_cat_count() }
    }

    fn rebuild(&mut self) {
        self.view = if !self.query.trim().is_empty() {
            data::search(&self.query)
        } else if self.cat == 0 {
            self.mine(&self.favs)
        } else if self.cat == 1 {
            self.mine(&self.recent)
        } else {
            data::category_items(self.kao, self.cat - 2)
        };
        let cols = ((CX1 - CX0) / EMOJI_CELL).floor() as usize;
        self.rows.clear();
        let mut y = 0.0;
        let mut i = 0;
        while i < self.view.len() {
            let kao = !data::is_emoji(self.view[i]);
            let (per, h) = if kao { (KAO_COLS, KAO_ROW) } else { (cols, EMOJI_CELL) };
            let mut len = 0;
            while len < per && i + len < self.view.len() && data::is_emoji(self.view[i + len]) != kao {
                len += 1;
            }
            self.rows.push(Row { y, h, start: i, len, kao });
            y += h;
            i += len;
        }
        self.content_h = y;
        self.sel = 0;
        self.scroll = 0.0;
    }

    fn row_of(&self, idx: usize) -> usize {
        self.rows.partition_point(|r| r.start + r.len <= idx)
    }

    fn view_h() -> f32 {
        CY1 - TOP
    }

    fn clamp_scroll(&mut self) {
        let max = (self.content_h - Self::view_h()).max(0.0);
        self.scroll = self.scroll.clamp(0.0, max);
    }

    /// Scrollbar thumb as (top, height), if the content overflows.
    fn thumb(&self) -> Option<(f32, f32)> {
        let vh = Self::view_h();
        (self.content_h > vh).then(|| {
            let h = (vh * vh / self.content_h).max(24.0);
            (TOP + (vh - h) * self.scroll / (self.content_h - vh), h)
        })
    }

    fn on_bar(x: f32, y: f32) -> bool {
        (CX1..CX1 + PAD).contains(&x) && (TOP..CY1).contains(&y)
    }

    fn on_close(x: f32, y: f32) -> bool {
        x >= WIDTH as f32 - 6.0 - CLOSE_W && y < TITLE_H
    }

    fn ensure_visible(&mut self) {
        if let Some(r) = self.rows.get(self.row_of(self.sel)) {
            if r.y < self.scroll {
                self.scroll = r.y;
            } else if r.y + r.h > self.scroll + Self::view_h() {
                self.scroll = r.y + r.h - Self::view_h();
            }
        }
    }

    fn move_sel_rows(&mut self, d: isize) {
        let r = self.row_of(self.sel);
        let col = self.sel - self.rows[r].start;
        let nr = (r as isize + d).clamp(0, self.rows.len() as isize - 1) as usize;
        let row = &self.rows[nr];
        self.sel = row.start + col.min(row.len - 1);
    }

    fn set_cat(&mut self, cat: usize) {
        self.cat = cat;
        self.query.clear();
        self.rebuild();
    }

    pub fn key(&mut self, sym: Keysym, utf8: Option<&str>, m: &Modifiers) -> Action {
        let has = !self.view.is_empty();
        match sym {
            Keysym::Escape => return Action::Close,
            Keysym::Return | Keysym::KP_Enter => {
                return if has { Action::Pick(self.view[self.sel]) } else { Action::None };
            }
            Keysym::Left if has => self.sel = self.sel.saturating_sub(1),
            Keysym::Right if has => self.sel = (self.sel + 1).min(self.view.len() - 1),
            Keysym::Up if has => self.move_sel_rows(-1),
            Keysym::Down if has => self.move_sel_rows(1),
            Keysym::Page_Up if has => self.move_sel_rows(-8),
            Keysym::Page_Down if has => self.move_sel_rows(8),
            Keysym::Home if has && self.query.is_empty() => self.sel = 0,
            Keysym::End if has && self.query.is_empty() => self.sel = self.view.len() - 1,
            Keysym::Tab | Keysym::ISO_Left_Tab if m.ctrl => {
                self.kao = !self.kao;
                self.set_cat(self.default_cat());
            }
            Keysym::Tab => self.set_cat((self.cat + 1) % self.cat_count()),
            Keysym::ISO_Left_Tab => self.set_cat((self.cat + self.cat_count() - 1) % self.cat_count()),
            Keysym::BackSpace => {
                if m.ctrl {
                    let t = self.query.trim_end().rfind(' ').map_or(0, |p| p + 1);
                    self.query.truncate(t);
                } else {
                    self.query.pop();
                }
                self.rebuild();
            }
            _ if m.ctrl && sym == Keysym::u => {
                self.query.clear();
                self.rebuild();
            }
            _ => match utf8 {
                Some(t) if !m.ctrl && !m.alt && !t.chars().any(char::is_control) && !t.is_empty() => {
                    self.query.push_str(t);
                    self.rebuild();
                }
                _ => return Action::None,
            },
        }
        self.ensure_visible();
        Action::Redraw
    }

    fn hit_content(&self, x: f32, y: f32) -> Option<usize> {
        if !(CX0..CX1).contains(&x) || !(TOP..CY1).contains(&y) {
            return None;
        }
        let cy = y - TOP + self.scroll;
        let r = self.rows.get(self.rows.partition_point(|r| r.y + r.h <= cy))?;
        let col = ((x - CX0) / r.cell_w()) as usize;
        (col < r.len).then_some(r.start + col)
    }

    pub fn motion(&mut self, x: f32, y: f32) -> Action {
        if let (Some(grab), Some((_, th))) = (self.bar_grab, self.thumb()) {
            let vh = Self::view_h();
            self.scroll = (y - grab - TOP) / (vh - th) * (self.content_h - vh);
            self.clamp_scroll();
            return Action::Redraw;
        }
        let state = (self.hit_content(x, y), Self::on_close(x, y), Self::on_bar(x, y));
        let changed = state != (self.hover, self.close_hover, self.bar_hover);
        (self.hover, self.close_hover, self.bar_hover) = state;
        Action::redraw_if(changed)
    }

    pub fn leave(&mut self) -> Action {
        (self.close_hover, self.bar_hover) = (false, false);
        Action::redraw_if(self.hover.take().is_some())
    }

    /// Left button released: ends a scrollbar drag.
    pub fn button_up(&mut self) -> Action {
        Action::redraw_if(self.bar_grab.take().is_some())
    }

    pub fn click(&mut self, x: f32, y: f32) -> Action {
        if let Some(i) = self.hit_content(x, y) {
            return Action::Pick(self.view[i]);
        }
        if Self::on_close(x, y) {
            return Action::Close;
        }
        if y < TITLE_H {
            return Action::StartMove;
        }
        if y < TITLE_H + SEARCH_H {
            return Action::FocusSearch;
        }
        if let (true, Some((ty, th))) = (Self::on_bar(x, y), self.thumb()) {
            // On the thumb: keep the grab point. On the track: center the thumb there.
            let grab = if (ty..ty + th).contains(&y) { y - ty } else { th / 2.0 };
            self.bar_grab = Some(grab);
            return self.motion(x, y);
        }
        if (PAD..SIDEBAR_W).contains(&x) && (TOP..TOP + MODE_H).contains(&y) {
            let kao = x >= (PAD + SIDEBAR_W) / 2.0;
            if kao != self.kao {
                self.kao = kao;
                self.set_cat(self.default_cat());
            }
            return Action::Redraw;
        }
        if (PAD..SIDEBAR_W).contains(&x) && y >= CAT_Y {
            let c = ((y - CAT_Y) / CAT_H) as usize;
            if c == 1 && x >= SIDEBAR_W - CLEAR_W && !self.mine(&self.recent).is_empty() {
                let kao = self.kao;
                self.recent.retain(|&i| data::is_emoji(i) == kao);
                save_list("recent", &self.recent);
                self.set_cat(1);
                return Action::Redraw;
            }
            if c < self.cat_count() {
                self.set_cat(c);
                return Action::Redraw;
            }
        }
        Action::None
    }

    pub fn scroll_by(&mut self, dy: f32) -> Action {
        let old = self.scroll;
        self.scroll += dy;
        self.clamp_scroll();
        Action::redraw_if(old != self.scroll)
    }

    pub fn add_recent(&mut self, item: u32) {
        self.recent.retain(|&i| i != item);
        self.recent.insert(0, item);
        self.recent.truncate(MAX_RECENT);
        save_list("recent", &self.recent);
    }

    /// Right click: add the item under the pointer to favorites, or remove it.
    pub fn right_click(&mut self, x: f32, y: f32) -> Action {
        let Some(i) = self.hit_content(x, y) else {
            return Action::None;
        };
        let item = self.view[i];
        match self.favs.iter().position(|&f| f == item) {
            Some(p) => {
                self.favs.remove(p);
            }
            None => self.favs.push(item),
        }
        save_list("favorites", &self.favs);
        if self.cat == 0 && self.query.trim().is_empty() {
            let scroll = self.scroll;
            self.rebuild();
            self.scroll = scroll;
            self.clamp_scroll();
        }
        Action::Redraw
    }

    // ---------------------------------------------------------------- drawing

    /// Draws the whole window into `pm` (physical pixels, `s` = scale factor).
    pub fn draw(&mut self, pm: &mut PixmapMut, s: f32) {
        let ncat = self.cat_count();
        let has_recent = !self.mine(&self.recent).is_empty();
        let fonts = self.fonts.get_or_insert_with(Fonts::new);
        fonts.scale = s;
        pm.fill(Color::TRANSPARENT);
        let (w, h) = (WIDTH as f32, HEIGHT as f32);
        fill_rr(pm, s, 0.0, 0.0, w, h, 12.0, BORDER, None);
        fill_rr(pm, s, 1.0, 1.0, w - 2.0, h - 2.0, 11.0, WINDOW, None);

        // Title bar.
        let ty = TITLE_H / 2.0 + 1.0;
        fonts.draw(pm, "Srvera Emoji", PAD + 4.0, ty, 12.0, DIM, 200.0, None);
        let cx = w - 6.0 - CLOSE_W;
        if self.close_hover {
            fill_rr(pm, s, cx, 4.0, CLOSE_W, TITLE_H - 6.0, 5.0, CLOSE, None);
        }
        let xw = fonts.measure("×", 18.0);
        let xc = if self.close_hover { TEXT } else { DIM };
        fonts.draw(pm, "×", cx + (CLOSE_W - xw) / 2.0, ty, 18.0, xc, CLOSE_W, None);

        // Search box.
        fill_rr(pm, s, PAD, TITLE_H, w - 2.0 * PAD, SEARCH_H, 8.0, ACCENT, None);
        fill_rr(pm, s, PAD + 1.5, TITLE_H + 1.5, w - 2.0 * PAD - 3.0, SEARCH_H - 3.0, 6.5, FIELD, None);
        let (tx, cy, maxw) = (PAD + 14.0, TITLE_H + SEARCH_H / 2.0, w - 2.0 * PAD - 28.0);
        let tw = if self.query.is_empty() {
            fonts.draw(pm, "Search emoji & kaomoji…", tx + 6.0, cy, 14.0, DIM, maxw, None);
            0.0
        } else {
            fonts.draw(pm, &self.query, tx, cy, 14.0, TEXT, maxw, None)
        };
        if self.caret {
            fill_rr(pm, s, tx + tw + 1.0, cy - 9.0, 1.5, 18.0, 0.0, TEXT, None);
        }

        // Emoji | Kaomoji switch.
        let half = (SIDEBAR_W - PAD) / 2.0;
        fill_rr(pm, s, PAD, TOP, SIDEBAR_W - PAD, MODE_H, 7.0, FIELD, None);
        let ax = if self.kao { PAD + half } else { PAD };
        fill_rr(pm, s, ax + 2.0, TOP + 2.0, half - 4.0, MODE_H - 4.0, 5.5, ACCENT, None);
        for (i, label) in ["Emoji", "Kaomoji"].iter().enumerate() {
            let x = PAD + half * i as f32 + (half - fonts.measure(label, UI_FONT)) / 2.0;
            let c = if (i == 1) == self.kao { FIELD } else { TEXT };
            fonts.draw(pm, label, x, TOP + MODE_H / 2.0, UI_FONT, c, half, None);
        }

        // Category list.
        let searching = !self.query.trim().is_empty();
        for c in 0..ncat {
            let y = CAT_Y + c as f32 * CAT_H;
            let (active, cy) = (c == self.cat && !searching, y + (CAT_H - 2.0) / 2.0);
            if active {
                fill_rr(pm, s, PAD, y, SIDEBAR_W - PAD, CAT_H - 2.0, 5.0, SEL, None);
            }
            let (name, icon) = match c {
                0 => ("Favorites", self.star),
                1 => ("Recent", self.recent_icon),
                _ => {
                    let (r, n) = data::category(self.kao, c - 2);
                    (n, Some(r.start))
                }
            };
            let mut x = PAD + 8.0;
            if let Some(icon) = icon.filter(|_| !self.kao) {
                fonts.emoji(pm, icon, x, cy - 8.0, 16.0, None);
                x += 22.0;
            }
            fonts.draw(pm, name, x, cy, UI_FONT, if active { TEXT } else { DIM }, SIDEBAR_W - x, None);
            if c == 1 && has_recent {
                let cw = fonts.measure("Clear", 11.0);
                fonts.draw(pm, "Clear", SIDEBAR_W - 8.0 - cw, cy, 11.0, DIM, CLEAR_W, None);
            }
        }

        // Content, clipped to its area.
        let (pw, ph) = (pm.width(), pm.height());
        if self.mask.as_ref().is_none_or(|m| m.width() != pw || m.height() != ph) {
            let mut m = Mask::new(pw, ph).unwrap();
            if let Some(r) = Rect::from_xywh(CX0 * s, TOP * s, (CX1 - CX0) * s, (CY1 - TOP) * s) {
                m.fill_path(&PathBuilder::from_rect(r), FillRule::Winding, false, Transform::identity());
            }
            self.mask = Some(m);
        }
        let mask = self.mask.as_ref();
        if self.view.is_empty() {
            let msg = if searching { "No results" } else { "Nothing here yet" };
            let x = CX0 + (CX1 - CX0 - fonts.measure(msg, 14.0)) / 2.0;
            fonts.draw(pm, msg, x, TOP + 60.0, 14.0, DIM, CX1 - CX0, None);
            return;
        }
        let first = self.rows.partition_point(|r| r.y + r.h <= self.scroll);
        for r in &self.rows[first..] {
            let y = TOP + r.y - self.scroll;
            if y >= CY1 {
                break;
            }
            let cw = r.cell_w();
            for k in 0..r.len {
                let (idx, x) = (r.start + k, CX0 + k as f32 * cw);
                let item = self.view[idx];
                let bg = if idx == self.sel { Some(SEL) } else { (Some(idx) == self.hover).then_some(HOVER) };
                if let Some(bg) = bg {
                    fill_rr(pm, s, x + 1.0, y + 1.0, cw - 2.0, r.h - 2.0, 6.0, bg, mask);
                }
                if r.kao {
                    fonts.draw(pm, data::text(item), x + 8.0, y + r.h / 2.0, KAO_FONT, TEXT, cw - 16.0, mask);
                } else {
                    let off = (EMOJI_CELL - EMOJI_SIZE) / 2.0;
                    fonts.emoji(pm, item, x + off, y + off, EMOJI_SIZE, mask);
                }
                // Small star in the corner of favorites.
                if let (true, Some(star)) = (self.favs.contains(&item), self.star) {
                    fonts.emoji(pm, star, x + cw - 14.0, y + 2.0, 12.0, mask);
                }
            }
        }

        // Scrollbar: wider and brighter while hovered or dragged.
        if let Some((ty, th)) = self.thumb() {
            let active = self.bar_hover || self.bar_grab.is_some();
            let (bx, bw, c) = if active { (CX1 + 2.0, 6.0, DIM) } else { (CX1 + 3.0, 4.0, BORDER) };
            fill_rr(pm, s, bx, ty, bw, th, 3.0, c, None);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn fill_rr(pm: &mut PixmapMut, s: f32, x: f32, y: f32, w: f32, h: f32, r: f32, rgba: u32, mask: Option<&Mask>) {
    let (x, y, w, h, r) = (x * s, y * s, w * s, h * s, (r * s).min(w / 2.0).min(h / 2.0));
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.quad_to(x + w, y, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.quad_to(x + w, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.quad_to(x, y + h, x, y + h - r);
    pb.line_to(x, y + r);
    pb.quad_to(x, y, x + r, y);
    pb.close();
    if let Some(p) = pb.finish() {
        pm.fill_path(&p, &paint(rgba), FillRule::Winding, Transform::identity(), mask);
    }
}

/// Saved lists live in ~/.local/share/srvera-emoji/<name>, one item per line.
fn list_path(name: &str) -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/share")))?;
    Some(base.join("srvera-emoji").join(name))
}

fn load_list(name: &str) -> Vec<u32> {
    let body = list_path(name).and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
    body.lines().filter_map(data::find_text).collect()
}

fn save_list(name: &str, items: &[u32]) {
    let Some(p) = list_path(name) else { return };
    let _ = std::fs::create_dir_all(p.parent().unwrap());
    let body: Vec<&str> = items.iter().map(|&i| data::text(i)).collect();
    let _ = std::fs::write(p, body.join("\n"));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Renders a few states to raw RGBA files for manual inspection:
    /// `cargo test --release snapshots -- --ignored`
    #[test]
    #[ignore]
    fn snapshots() {
        let dir = std::env::var("SNAP_DIR").unwrap_or_else(|_| "target".into());
        let mut ui = Ui::new();
        let t = std::time::Instant::now();
        ui.warm_up();
        eprintln!("warm_up: {:?}", t.elapsed());
        ui.reset();
        let m = Modifiers::default();
        let ctrl = Modifiers { ctrl: true, ..Default::default() };
        let shot = |ui: &mut Ui, name: &str| {
            let (w, h) = (WIDTH, HEIGHT);
            let mut buf = vec![0u8; (w * h * 4) as usize];
            let mut pm = PixmapMut::from_bytes(&mut buf, w, h).unwrap();
            let t = std::time::Instant::now();
            ui.draw(&mut pm, 1.0);
            eprintln!("{name}: {:?}", t.elapsed());
            std::fs::write(format!("{dir}/{name}.rgba"), &buf).unwrap();
        };
        // Emoji mode: a few favorites and recents (not saved to disk).
        let pick = |t: &str| data::find_text(t).unwrap();
        ui.favs = vec![pick("😀"), pick("😂"), pick("(╥﹏╥)")];
        ui.recent = vec![pick("😂"), pick("👍")];
        ui.set_cat(2);
        shot(&mut ui, "emoji_stars");
        ui.set_cat(0);
        shot(&mut ui, "emoji_favorites");
        ui.key(Keysym::Tab, None, &ctrl); // -> kaomoji, opens on Favorites
        shot(&mut ui, "kaomoji_favorites");
        ui.set_cat(5); // Crying
        shot(&mut ui, "kaomoji_crying");
        ui.set_cat(13); // Seasonal: many emojis inside kaomojis
        shot(&mut ui, "kaomoji_seasonal");
        for c in ["c", "r", "y"] {
            ui.key(Keysym::NoSymbol, Some(c), &m);
        }
        ui.caret = true;
        shot(&mut ui, "search_cry");
    }
}
