//! Picker state, layout, drawing and input handling. Knows nothing about Wayland:
//! it draws into a pixel buffer and turns key/pointer input into `Action`s.

use crate::{data, emoji_img::EmojiCache, text::Fonts};
use resvg::tiny_skia::{
    Color, FillRule, Mask, Paint, PathBuilder, PixmapMut, PixmapPaint, Rect, Transform,
};
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
const CAT_H: f32 = 23.0;
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

fn rgb(hex: u32, a: u8) -> Color {
    Color::from_rgba8((hex >> 16) as u8, (hex >> 8) as u8, hex as u8, a)
}
// Breeze Dark-like palette.
fn c_window() -> Color { rgb(0x202326, 0xFF) }
fn c_field() -> Color { rgb(0x141618, 0xFF) }
fn c_border() -> Color { rgb(0x3A3F44, 0xFF) }
fn c_text() -> Color { rgb(0xFCFCFC, 0xFF) }
fn c_dim() -> Color { rgb(0x9AA1A8, 0xFF) }
fn c_accent() -> Color { rgb(0x3DAEE9, 0xFF) }
fn c_sel() -> Color { rgb(0x3DAEE9, 0x60) }
fn c_hover() -> Color { rgb(0xFFFFFF, 0x16) }
fn c_close() -> Color { rgb(0xDA4453, 0xFF) }

pub enum Action {
    None,
    Redraw,
    Close,
    Pick(u32),
    StartMove,
    FocusSearch,
}

struct Row {
    y: f32,
    h: f32,
    start: usize,
    len: usize,
    kao: bool,
}

pub struct Ui {
    pub query: String,
    kao: bool,
    cat: usize, // 0 = Recent, n = category n-1
    view: Vec<u32>,
    rows: Vec<Row>,
    content_h: f32,
    sel: usize,
    hover: Option<usize>,
    scroll: f32,
    recent: Vec<u32>,
    fonts: Option<Fonts>,
    emojis: EmojiCache,
    mask: Option<Mask>,
    recent_icon: Option<u32>,
    close_hover: bool,
    bar_hover: bool,
    bar_grab: Option<f32>, // pointer offset inside the scrollbar thumb while dragging
}

impl Ui {
    pub fn new() -> Self {
        let mut ui = Ui {
            query: String::new(),
            kao: false,
            cat: 1,
            view: Vec::new(),
            rows: Vec::new(),
            content_h: 0.0,
            sel: 0,
            hover: None,
            scroll: 0.0,
            recent: load_recent(),
            fonts: None,
            emojis: EmojiCache::default(),
            mask: None,
            recent_icon: data::find_text(RECENT_ICON),
            close_hover: false,
            bar_hover: false,
            bar_grab: None,
        };
        if !ui.recent.is_empty() {
            ui.cat = 0;
        }
        ui
    }

    /// Loads the font list ahead of the first open so the window appears instantly.
    pub fn warm_up(&mut self) {
        self.fonts.get_or_insert_with(Fonts::new);
    }

    /// Called every time the window opens.
    pub fn reset(&mut self) {
        self.query.clear();
        self.hover = None;
        self.close_hover = false;
        self.bar_hover = false;
        self.bar_grab = None;
        self.rebuild();
    }

    /// Frees everything that is only needed while the window is visible.
    pub fn release(&mut self) {
        if let Some(f) = &mut self.fonts {
            f.release();
        }
        self.emojis.clear();
        self.mask = None;
        self.view = Vec::new();
        self.rows = Vec::new();
    }

    fn cat_count(&self) -> usize {
        1 + if self.kao { data::kao_cat_count() } else { data::emoji_cat_count() }
    }

    fn rebuild(&mut self) {
        self.view = if !self.query.trim().is_empty() {
            data::search(&self.query)
        } else if self.cat == 0 {
            self.recent.iter().copied().filter(|&i| data::is_emoji(i) != self.kao).collect()
        } else {
            data::category_items(self.kao, self.cat - 1)
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
                self.set_cat(1);
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
        let cw = if r.kao { (CX1 - CX0) / KAO_COLS as f32 } else { EMOJI_CELL };
        let col = ((x - CX0) / cw) as usize;
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
        if state != (self.hover, self.close_hover, self.bar_hover) {
            (self.hover, self.close_hover, self.bar_hover) = state;
            return Action::Redraw;
        }
        Action::None
    }

    pub fn leave(&mut self) -> Action {
        self.close_hover = false;
        self.bar_hover = false;
        self.hover.take().map_or(Action::None, |_| Action::Redraw)
    }

    /// Left button released: ends a scrollbar drag.
    pub fn button_up(&mut self) -> Action {
        self.bar_grab.take().map_or(Action::None, |_| Action::Redraw)
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
                self.set_cat(1);
            }
            return Action::Redraw;
        }
        if (PAD..SIDEBAR_W).contains(&x) && y >= CAT_Y {
            let c = ((y - CAT_Y) / CAT_H) as usize;
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
        if old != self.scroll { Action::Redraw } else { Action::None }
    }

    pub fn add_recent(&mut self, item: u32) {
        self.recent.retain(|&i| i != item);
        self.recent.insert(0, item);
        self.recent.truncate(MAX_RECENT);
        save_recent(&self.recent);
    }

    // ---------------------------------------------------------------- drawing

    /// Draws the whole window into `pm` (physical pixels, `s` = scale factor).
    pub fn draw(&mut self, pm: &mut PixmapMut, s: f32) {
        let ncat = self.cat_count();
        let fonts = self.fonts.get_or_insert_with(Fonts::new);
        pm.fill(Color::TRANSPARENT);
        let (w, h) = (WIDTH as f32, HEIGHT as f32);
        fill_rr(pm, s, 0.0, 0.0, w, h, 12.0, c_border(), None);
        fill_rr(pm, s, 1.0, 1.0, w - 2.0, h - 2.0, 11.0, c_window(), None);

        // Title bar.
        fonts.draw(pm, "Emoji Picker", (PAD + 4.0) * s, (TITLE_H / 2.0 + 1.0) * s, 12.0 * s, c_dim(), 200.0 * s, None);
        let cx = w - 6.0 - CLOSE_W;
        if self.close_hover {
            fill_rr(pm, s, cx, 4.0, CLOSE_W, TITLE_H - 6.0, 5.0, c_close(), None);
        }
        let xw = fonts.measure("×", 18.0 * s) / s;
        let xc = if self.close_hover { c_text() } else { c_dim() };
        fonts.draw(pm, "×", (cx + (CLOSE_W - xw) / 2.0) * s, (TITLE_H / 2.0 + 1.0) * s, 18.0 * s, xc, CLOSE_W * s, None);

        // Search box.
        fill_rr(pm, s, PAD, TITLE_H, w - 2.0 * PAD, SEARCH_H, 8.0, c_accent(), None);
        fill_rr(pm, s, PAD + 1.5, TITLE_H + 1.5, w - 2.0 * PAD - 3.0, SEARCH_H - 3.0, 6.5, c_field(), None);
        let cy = TITLE_H + SEARCH_H / 2.0;
        let tx = PAD + 14.0;
        let maxw = (w - 2.0 * PAD - 28.0) * s;
        let tw = if self.query.is_empty() {
            fonts.draw(pm, "Search emoji & kaomoji…", (tx + 6.0) * s, cy * s, 14.0 * s, c_dim(), maxw, None);
            0.0
        } else {
            fonts.draw(pm, &self.query, tx * s, cy * s, 14.0 * s, c_text(), maxw, None) / s
        };
        fill_rr(pm, s, tx + tw + 1.0, cy - 9.0, 1.5, 18.0, 0.0, c_text(), None);

        // Emoji | Kaomoji switch.
        let half = (SIDEBAR_W - PAD) / 2.0;
        fill_rr(pm, s, PAD, TOP, SIDEBAR_W - PAD, MODE_H, 7.0, c_field(), None);
        let ax = if self.kao { PAD + half } else { PAD };
        fill_rr(pm, s, ax + 2.0, TOP + 2.0, half - 4.0, MODE_H - 4.0, 5.5, c_accent(), None);
        for (i, label) in ["Emoji", "Kaomoji"].iter().enumerate() {
            let lw = fonts.measure(label, UI_FONT * s) / s;
            let x = PAD + half * i as f32 + (half - lw) / 2.0;
            let col = if (i == 1) == self.kao { c_field() } else { c_text() };
            fonts.draw(pm, label, x * s, (TOP + MODE_H / 2.0) * s, UI_FONT * s, col, half * s, None);
        }

        // Category list.
        let searching = !self.query.trim().is_empty();
        for c in 0..ncat {
            let y = CAT_Y + c as f32 * CAT_H;
            if c == self.cat && !searching {
                fill_rr(pm, s, PAD, y, SIDEBAR_W - PAD, CAT_H - 2.0, 5.0, c_sel(), None);
            }
            let (name, icon) = if c == 0 {
                ("Recent", self.recent_icon.filter(|_| !self.kao))
            } else {
                let (r, n) = data::category(self.kao, c - 1);
                (n, (!self.kao).then_some(r.start))
            };
            let mut x = PAD + 8.0;
            if let Some(icon) = icon {
                let px = (16.0 * s).round() as u32;
                if let Some(img) = self.emojis.get(icon, px) {
                    let iy = y + (CAT_H - 2.0 - 16.0) / 2.0;
                    pm.draw_pixmap((x * s) as i32, (iy * s) as i32, img.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
                }
                x += 22.0;
            }
            let col = if c == self.cat && !searching { c_text() } else { c_dim() };
            fonts.draw(pm, name, x * s, (y + (CAT_H - 2.0) / 2.0) * s, UI_FONT * s, col, (SIDEBAR_W - x) * s, None);
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
            let mw = fonts.measure(msg, 14.0 * s) / s;
            let x = CX0 + (CX1 - CX0 - mw) / 2.0;
            fonts.draw(pm, msg, x * s, (TOP + 60.0) * s, 14.0 * s, c_dim(), (CX1 - CX0) * s, None);
            return;
        }
        let first = self.rows.partition_point(|r| r.y + r.h <= self.scroll);
        for r in &self.rows[first..] {
            let y = TOP + r.y - self.scroll;
            if y >= CY1 {
                break;
            }
            let cw = if r.kao { (CX1 - CX0) / KAO_COLS as f32 } else { EMOJI_CELL };
            for k in 0..r.len {
                let idx = r.start + k;
                let item = self.view[idx];
                let x = CX0 + k as f32 * cw;
                let bg = if idx == self.sel { Some(c_sel()) } else if Some(idx) == self.hover { Some(c_hover()) } else { None };
                if let Some(bg) = bg {
                    fill_rr(pm, s, x + 1.0, y + 1.0, cw - 2.0, r.h - 2.0, 6.0, bg, mask);
                }
                if r.kao {
                    let t = data::text(item);
                    fonts.draw(pm, t, (x + 8.0) * s, (y + r.h / 2.0) * s, KAO_FONT * s, c_text(), (cw - 16.0) * s, mask);
                } else {
                    let px = (EMOJI_SIZE * s).round() as u32;
                    if let Some(img) = self.emojis.get(item, px) {
                        let off = (EMOJI_CELL - EMOJI_SIZE) / 2.0;
                        pm.draw_pixmap(((x + off) * s).round() as i32, ((y + off) * s).round() as i32, img.as_ref(), &PixmapPaint::default(), Transform::identity(), mask);
                    }
                }
            }
        }

        // Scrollbar: wider and brighter while hovered or dragged.
        if let Some((ty, th)) = self.thumb() {
            let active = self.bar_hover || self.bar_grab.is_some();
            let (bx, bw, c) = if active { (CX1 + 2.0, 6.0, c_dim()) } else { (CX1 + 3.0, 4.0, c_border()) };
            fill_rr(pm, s, bx, ty, bw, th, 3.0, c, None);
        }
    }

}

#[allow(clippy::too_many_arguments)]
fn fill_rr(pm: &mut PixmapMut, s: f32, x: f32, y: f32, w: f32, h: f32, r: f32, c: Color, mask: Option<&Mask>) {
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
        let mut paint = Paint::default();
        paint.set_color(c);
        paint.anti_alias = true;
        pm.fill_path(&p, &paint, FillRule::Winding, Transform::identity(), mask);
    }
}

fn recent_path() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".cache")))?;
    Some(base.join("emoji-picker/recent"))
}

fn load_recent() -> Vec<u32> {
    let Some(p) = recent_path() else { return Vec::new() };
    std::fs::read_to_string(p)
        .unwrap_or_default()
        .lines()
        .filter_map(data::find_text)
        .take(MAX_RECENT)
        .collect()
}

fn save_recent(items: &[u32]) {
    let Some(p) = recent_path() else { return };
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
        ui.reset();
        let m = Modifiers::default();
        let ctrl = Modifiers { ctrl: true, ..Default::default() };
        let mut shot = |ui: &mut Ui, name: &str| {
            let (w, h) = (WIDTH, HEIGHT);
            let mut buf = vec![0u8; (w * h * 4) as usize];
            let mut pm = PixmapMut::from_bytes(&mut buf, w, h).unwrap();
            let t = std::time::Instant::now();
            ui.draw(&mut pm, 1.0);
            eprintln!("{name}: {:?}", t.elapsed());
            std::fs::write(format!("{dir}/{name}.rgba"), &buf).unwrap();
        };
        ui.key(Keysym::Tab, None, &ctrl); // -> kaomoji
        shot(&mut ui, "kaomoji");
        ui.key(Keysym::Tab, None, &m);
        ui.key(Keysym::Tab, None, &m); // -> Crying
        ui.key(Keysym::Tab, None, &m);
        shot(&mut ui, "kaomoji_crying");
        for c in ["c", "r", "y"] {
            ui.key(Keysym::NoSymbol, Some(c), &m);
        }
        shot(&mut ui, "search_cry");
    }
}
