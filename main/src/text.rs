//! Text drawing: per-character font fallback (fontdb), shaping (rustybuzz) and
//! glyph outlines filled straight into the frame with tiny-skia. No glyph cache:
//! a frame only shows a few hundred glyphs, and this keeps memory flat.

use resvg::tiny_skia::{Color, FillRule, Mask, Paint, PathBuilder, PixmapMut, Transform};
use rustybuzz::ttf_parser;
use std::collections::HashMap;

/// Tried in this order before scanning every installed font.
const PREFERRED: &[&str] = &[
    "Noto Sans", "Noto Sans CJK JP", "Noto Sans Symbols", "Noto Sans Symbols 2",
    "Noto Sans Math", "DejaVu Sans", "Noto Sans Thai", "Noto Sans Kannada", "Noto Sans Tamil",
    "Noto Sans Arabic", "Noto Sans Devanagari", "Noto Sans Telugu", "Noto Sans Bengali",
    "Noto Sans Sinhala", "Noto Sans Ethiopic", "Noto Sans Canadian Aboriginal",
    "Noto Sans Mongolian", "Noto Sans Georgian", "Noto Sans Armenian", "Noto Sans Hebrew",
];

/// A memory-mapped font file and the parsed face inside it.
struct Mapped {
    face: rustybuzz::Face<'static>, // must drop before the mapping below
    ptr: *mut libc::c_void,
    len: usize,
}

impl Drop for Mapped {
    fn drop(&mut self) {
        unsafe { libc::munmap(self.ptr, self.len) };
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
            Some(face) => Some(Mapped { face, ptr, len }),
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
}

struct Glyph {
    face: fontdb::ID,
    id: u16,
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
            .iter()
            .filter_map(|f| {
                db.query(&fontdb::Query { families: &[fontdb::Family::Name(f)], ..Default::default() })
            })
            .collect();
        Fonts { db, chain, char_face: HashMap::new(), loaded: HashMap::new() }
    }

    /// Unmap all font files (called when the window hides).
    pub fn release(&mut self) {
        self.loaded.clear();
        self.loaded.shrink_to_fit();
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
        self.loaded.get(&id).map(|m| &m.face)
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
        if let Some(&f) = self.char_face.get(&c) {
            return f;
        }
        let mut found = None;
        for i in 0..self.chain.len() {
            let id = self.chain[i];
            if self.covers(id, c) {
                found = Some(id);
                break;
            }
        }
        if found.is_none() {
            // Slow path, once per character: scan every installed outline font.
            let ids: Vec<fontdb::ID> = self
                .db
                .faces()
                .filter(|f| !f.families.iter().any(|(n, _)| n.contains("Emoji")))
                .map(|f| f.id)
                .collect();
            found = ids.into_iter().find(|&id| {
                self.db
                    .with_face_data(id, |d, i| {
                        ttf_parser::Face::parse(d, i)
                            .ok()
                            .and_then(|f| f.glyph_index(c))
                            .is_some_and(|g| g.0 != 0)
                    })
                    .unwrap_or(false)
            });
        }
        self.char_face.insert(c, found);
        found.or_else(|| prev.or(self.chain.first().copied()))
    }

    /// Shapes `s` at `size` px. Returns positioned glyphs and the total advance.
    fn layout(&mut self, s: &str, size: f32) -> (Vec<Glyph>, f32) {
        // Split into runs that use the same font.
        let mut runs: Vec<(fontdb::ID, usize, usize)> = Vec::new();
        let mut prev = None;
        for (i, c) in s.char_indices() {
            let Some(f) = self.face_for(c, prev) else { continue };
            match runs.last_mut() {
                Some(r) if r.0 == f && r.2 == i => r.2 = i + c.len_utf8(),
                _ => runs.push((f, i, i + c.len_utf8())),
            }
            prev = Some(f);
        }
        let mut glyphs = Vec::new();
        let mut x = 0.0;
        for (id, a, b) in runs {
            let Some(face) = self.face(id) else { continue };
            let scale = size / face.units_per_em() as f32;
            let mut buf = rustybuzz::UnicodeBuffer::new();
            buf.push_str(&s[a..b]);
            buf.guess_segment_properties();
            let out = rustybuzz::shape(face, &[], buf);
            for (info, pos) in out.glyph_infos().iter().zip(out.glyph_positions()) {
                glyphs.push(Glyph {
                    face: id,
                    id: info.glyph_id as u16,
                    x: x + pos.x_offset as f32 * scale,
                    y: pos.y_offset as f32 * scale,
                    scale,
                });
                x += pos.x_advance as f32 * scale;
            }
        }
        (glyphs, x)
    }

    pub fn measure(&mut self, s: &str, size: f32) -> f32 {
        self.layout(s, size).1
    }

    /// Draws `s` with its left edge at `x`, vertically centered on `cy` (all physical px).
    /// Text wider than `max_w` is shrunk (down to 70%) and then cut off.
    /// Returns the drawn width.
    pub fn draw(
        &mut self, pm: &mut PixmapMut, s: &str, x: f32, cy: f32, size: f32, color: Color, max_w: f32,
        mask: Option<&Mask>,
    ) -> f32 {
        let (mut glyphs, mut w) = self.layout(s, size);
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
        let mut paint = Paint::default();
        paint.set_color(color);
        paint.anti_alias = true;
        for g in &glyphs {
            if g.x > max_w {
                break;
            }
            let Some(face) = self.face(g.face) else { continue };
            let mut b = Outline { pb: PathBuilder::new(), ox: x + g.x, oy: baseline - g.y, s: g.scale };
            if face.outline_glyph(ttf_parser::GlyphId(g.id), &mut b).is_some() {
                if let Some(path) = b.pb.finish() {
                    pm.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), mask);
                }
            }
        }
        w.min(max_w)
    }
}

struct Outline {
    pb: PathBuilder,
    ox: f32,
    oy: f32,
    s: f32,
}

impl ttf_parser::OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.pb.move_to(self.ox + x * self.s, self.oy - y * self.s);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.pb.line_to(self.ox + x * self.s, self.oy - y * self.s);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.pb.quad_to(self.ox + x1 * self.s, self.oy - y1 * self.s, self.ox + x * self.s, self.oy - y * self.s);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.pb.cubic_to(
            self.ox + x1 * self.s, self.oy - y1 * self.s,
            self.ox + x2 * self.s, self.oy - y2 * self.s,
            self.ox + x * self.s, self.oy - y * self.s,
        );
    }
    fn close(&mut self) {
        self.pb.close();
    }
}
