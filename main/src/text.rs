//! Text drawing: per-character font fallback (fontdb), shaping (rustybuzz) and
//! glyph outlines filled straight into the frame with tiny-skia. No glyph cache:
//! a frame only shows a few hundred glyphs, and this keeps memory flat.
//! Emojis inside text are drawn from the Twemoji SVGs, like in the grid.

use crate::{data, emoji_img::EmojiCache};
use resvg::tiny_skia::{FillRule, Mask, Paint, PathBuilder, PixmapMut, PixmapPaint, Transform};
use rustybuzz::ttf_parser;
use std::collections::HashMap;

/// Tried in this order before scanning every installed font (comma separated).
const PREFERRED: &str = "Noto Sans,Noto Sans CJK JP,Noto Sans Symbols,Noto Sans Symbols 2,Noto Sans Math,DejaVu Sans,\
    Noto Sans Thai,Noto Sans Kannada,Noto Sans Tamil,Noto Sans Arabic,Noto Sans Devanagari,Noto Sans Telugu,\
    Noto Sans Bengali,Noto Sans Sinhala,Noto Sans Ethiopic,Noto Sans Canadian Aboriginal,Noto Sans Mongolian,\
    Noto Sans Georgian,Noto Sans Armenian,Noto Sans Hebrew";

/// A solid paint from an RGBA color like `0x3DAEE9FF`.
pub fn paint(rgba: u32) -> Paint<'static> {
    let [r, g, b, a] = rgba.to_be_bytes();
    let mut p = Paint::default();
    p.set_color_rgba8(r, g, b, a);
    p
}

/// A memory-mapped font file and the parsed face inside it.
struct Mapped {
    face: std::mem::ManuallyDrop<rustybuzz::Face<'static>>, // dropped before the unmap
    ptr: *mut libc::c_void,
    len: usize,
}

impl Drop for Mapped {
    fn drop(&mut self) {
        unsafe {
            std::mem::ManuallyDrop::drop(&mut self.face);
            libc::munmap(self.ptr, self.len);
        }
    }
}

fn map_face(path: &std::path::Path, index: u32) -> Option<Mapped> {
    use std::os::fd::AsRawFd;
    let file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len() as usize;
    unsafe {
        let ptr = libc::mmap(std::ptr::null_mut(), len, libc::PROT_READ, libc::MAP_PRIVATE, file.as_raw_fd(), 0);
        if ptr == libc::MAP_FAILED {
            return None;
        }
        let data: &'static [u8] = std::slice::from_raw_parts(ptr as *const u8, len);
        match rustybuzz::Face::from_slice(data, index) {
            Some(face) => Some(Mapped { face: std::mem::ManuallyDrop::new(face), ptr, len }),
            None => {
                libc::munmap(ptr, len);
                None
            }
        }
    }
}

pub struct Fonts {
    db: fontdb::Database,
    chain: Vec<fontdb::ID>,
    char_face: HashMap<char, Option<fontdb::ID>>,
    loaded: HashMap<fontdb::ID, Mapped>,
    pub emojis: EmojiCache,
    pub scale: f32, // draw/measure/emoji take logical px and multiply by this
}

struct Glyph {
    face: Option<fontdb::ID>, // None: an emoji, `id` is its item and `scale` its size in px
    id: u32,
    x: f32, // in font units * scale already applied (px)
    y: f32,
    scale: f32,
}

/// Rough "combining mark" test so accents stay in the font of the letter they sit on.
fn is_mark(c: char) -> bool {
    matches!(c as u32,
        0x0300..=0x036F | 0x0483..=0x0489 | 0x0591..=0x05BD | 0x0610..=0x061A | 0x064B..=0x065F
        | 0x0E31 | 0x0E34..=0x0E3A | 0x0E47..=0x0E4E | 0x0C81..=0x0C83 | 0x0CBC..=0x0CCD
        | 0x0B82 | 0x0BBE..=0x0BCD | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x20D0..=0x20FF
        | 0x302A..=0x302F | 0x3099..=0x309A | 0xFE20..=0xFE2F | 0xFE00..=0xFE0F)
}

impl Fonts {
    pub fn new() -> Self {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        let chain = PREFERRED
            .split(',')
            .filter_map(|f| db.query(&fontdb::Query { families: &[fontdb::Family::Name(f)], ..Default::default() }))
            .collect();
        Fonts {
            db,
            chain,
            char_face: HashMap::new(),
            loaded: HashMap::new(),
            emojis: EmojiCache::default(),
            scale: 1.0,
        }
    }

    /// Unmap all font files and drop emoji images (called when the window hides).
    pub fn release(&mut self) {
        self.loaded.clear();
        self.loaded.shrink_to_fit();
        self.emojis.clear();
    }

    fn face(&mut self, id: fontdb::ID) -> Option<&rustybuzz::Face<'static>> {
        if !self.loaded.contains_key(&id) {
            let (src, index) = self.db.face_source(id)?;
            let path = match &src {
                fontdb::Source::File(p) => p.clone(),
                fontdb::Source::SharedFile(p, _) => p.clone(),
                _ => return None,
            };
            let m = map_face(&path, index)?;
            self.loaded.insert(id, m);
        }
        self.loaded.get(&id).map(|m| &*m.face)
    }

    fn covers(&mut self, id: fontdb::ID, c: char) -> bool {
        self.face(id).is_some_and(|f| f.glyph_index(c).is_some_and(|g| g.0 != 0))
    }

    fn face_for(&mut self, c: char, prev: Option<fontdb::ID>) -> Option<fontdb::ID> {
        if let Some(p) = prev {
            if (is_mark(c) || c == '\u{200D}') && self.covers(p, c) {
                return Some(p);
            }
        }
        if !self.char_face.contains_key(&c) {
            self.prefetch(std::iter::once(c));
        }
        self.char_face[&c].or_else(|| prev.or(self.chain.first().copied()))
    }

    /// Finds a font for every char in `chars`: the preferred fonts first, then a single
    /// pass over every installed font for all the rest (one pass per char used to stutter).
    pub fn prefetch(&mut self, chars: impl Iterator<Item = char>) {
        let mut todo = Vec::new();
        for c in chars {
            if self.char_face.contains_key(&c) {
                continue;
            }
            let found = self.chain.clone().into_iter().find(|&id| self.covers(id, c));
            self.char_face.insert(c, found);
            if found.is_none() {
                todo.push(c);
            }
        }
        let ids: Vec<fontdb::ID> =
            self.db.faces().filter(|f| !f.families.iter().any(|(n, _)| n.contains("Emoji"))).map(|f| f.id).collect();
        for id in ids {
            if todo.is_empty() {
                break;
            }
            let char_face = &mut self.char_face;
            self.db.with_face_data(id, |d, i| {
                let Ok(f) = ttf_parser::Face::parse(d, i) else { return };
                todo.retain(|&c| {
                    let hit = f.glyph_index(c).is_some_and(|g| g.0 != 0);
                    if hit {
                        char_face.insert(c, Some(id));
                    }
                    !hit
                });
            });
        }
    }

    /// Shapes `s` at `size` px. Returns positioned glyphs and the total advance.
    fn layout(&mut self, s: &str, size: f32) -> (Vec<Glyph>, f32) {
        // Split into runs that use the same font; each emoji is a run of its own (None).
        let mut runs: Vec<(Option<fontdb::ID>, usize, usize)> = Vec::new();
        let (mut prev, mut i) = (None, 0);
        while let Some(c) = s[i..].chars().next() {
            if let Some((_, n)) = data::emoji_prefix(&s[i..]) {
                runs.push((None, i, i + n));
                i += n;
                continue;
            }
            let n = c.len_utf8();
            if let Some(f) = self.face_for(c, prev) {
                match runs.last_mut() {
                    Some(r) if r.0 == Some(f) && r.2 == i => r.2 = i + n,
                    _ => runs.push((Some(f), i, i + n)),
                }
                prev = Some(f);
            }
            i += n;
        }
        let mut glyphs = Vec::new();
        let mut x = 0.0;
        for (id, a, b) in runs {
            let Some(id) = id else {
                let item = data::emoji_prefix(&s[a..]).map_or(0, |e| e.0);
                glyphs.push(Glyph { face: None, id: item, x: x + size * 0.1, y: 0.0, scale: size });
                x += size * 1.2;
                continue;
            };
            let Some(face) = self.face(id) else { continue };
            let scale = size / face.units_per_em() as f32;
            let mut buf = rustybuzz::UnicodeBuffer::new();
            buf.push_str(&s[a..b]);
            buf.guess_segment_properties();
            let out = rustybuzz::shape(face, &[], buf);
            for (info, pos) in out.glyph_infos().iter().zip(out.glyph_positions()) {
                glyphs.push(Glyph {
                    face: Some(id),
                    id: info.glyph_id,
                    x: x + pos.x_offset as f32 * scale,
                    y: pos.y_offset as f32 * scale,
                    scale,
                });
                x += pos.x_advance as f32 * scale;
            }
        }
        (glyphs, x)
    }

    pub fn measure(&mut self, t: &str, size: f32) -> f32 {
        self.layout(t, size * self.scale).1 / self.scale
    }

    /// Draws emoji `item`, `size` px big, with its top-left corner at (`x`, `y`).
    pub fn emoji(&mut self, pm: &mut PixmapMut, item: u32, x: f32, y: f32, size: f32, mask: Option<&Mask>) {
        let s = self.scale;
        self.blit(pm, item, (size * s).round() as u32, x * s, y * s, mask);
    }

    /// Like `emoji`, in physical px.
    fn blit(&mut self, pm: &mut PixmapMut, item: u32, px: u32, x: f32, y: f32, mask: Option<&Mask>) {
        if let Some(img) = self.emojis.get(item, px) {
            let (x, y) = (x.round() as i32, y.round() as i32);
            pm.draw_pixmap(x, y, img.as_ref(), &PixmapPaint::default(), Transform::identity(), mask);
        }
    }

    /// Draws `t` with its left edge at `x`, vertically centered on `cy`. Text wider than
    /// `max_w` is shrunk (down to 70%) and then cut off. Returns the drawn width.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        pm: &mut PixmapMut,
        t: &str,
        x: f32,
        cy: f32,
        size: f32,
        rgba: u32,
        max_w: f32,
        mask: Option<&Mask>,
    ) -> f32 {
        let sc = self.scale;
        let (x, cy, size, max_w) = (x * sc, cy * sc, size * sc, max_w * sc);
        let (mut glyphs, mut w) = self.layout(t, size);
        if w > max_w && w > 0.0 {
            let k = (max_w / w).max(0.7);
            for g in &mut glyphs {
                g.x *= k;
                g.y *= k;
                g.scale *= k;
            }
            w *= k;
        }
        let baseline = cy + size * 0.36;
        let paint = paint(rgba);
        for g in &glyphs {
            if g.x > max_w {
                break;
            }
            let Some(fid) = g.face else {
                self.blit(pm, g.id, g.scale.round() as u32, x + g.x, cy - g.scale / 2.0, mask);
                continue;
            };
            let Some(face) = self.face(fid) else { continue };
            let mut b = Outline { pb: PathBuilder::new(), ox: x + g.x, oy: baseline - g.y, s: g.scale };
            if face.outline_glyph(ttf_parser::GlyphId(g.id as u16), &mut b).is_some() {
                if let Some(path) = b.pb.finish() {
                    pm.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), mask);
                }
            }
        }
        w.min(max_w) / sc
    }
}

struct Outline {
    pb: PathBuilder,
    ox: f32,
    oy: f32,
    s: f32,
}

impl Outline {
    fn at(&self, x: f32, y: f32) -> (f32, f32) {
        (self.ox + x * self.s, self.oy - y * self.s)
    }
}

impl ttf_parser::OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.at(x, y);
        self.pb.move_to(x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.at(x, y);
        self.pb.line_to(x, y);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let ((ax, ay), (x, y)) = (self.at(x1, y1), self.at(x, y));
        self.pb.quad_to(ax, ay, x, y);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let ((ax, ay), (bx, by), (x, y)) = (self.at(x1, y1), self.at(x2, y2), self.at(x, y));
        self.pb.cubic_to(ax, ay, bx, by, x, y);
    }
    fn close(&mut self) {
        self.pb.close();
    }
}
