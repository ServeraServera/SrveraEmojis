//! Read-only access to the tables built by build.rs, plus keyword search.
//!
//! data.bin layout (little endian):
//!   header: n_items, n_kw, n_emoji_cats, n_kao_cats, n_emojis   (u32 each)
//!   items:  n_items × { text_off u32, text_len u16 (bit 15 = skin tone), kw u16, svg_off u32, svg_len u32 }
//!   kws:    n_kw × { off u32, len u32 }
//!   cats:   (n_emoji_cats + n_kao_cats) × { start u32, end u32, name_off u32, name_len u32 }
//!   sorted: n_emojis × u16 emoji ids, sorted by text
//! Emojis come first; each category is a contiguous range of items.

const DATA: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/data.bin"));
const STRINGS: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/strings.bin"));
const SVGS: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/svgs.bin"));
pub const KAO_CHARS: &str = include_str!(concat!(env!("OUT_DIR"), "/kao_chars.txt"));

const HEADER: usize = 20;
const ITEM: usize = 16;

fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes(b[o..o + 2].try_into().unwrap())
}
fn string(off: u32, len: usize) -> &'static str {
    // Written from valid &str slices by build.rs.
    std::str::from_utf8(&STRINGS[off as usize..off as usize + len]).unwrap_or("")
}

pub fn item_count() -> u32 {
    u32_at(DATA, 0)
}
fn kw_count() -> usize {
    u32_at(DATA, 4) as usize
}
pub fn emoji_cat_count() -> usize {
    u32_at(DATA, 8) as usize
}
pub fn kao_cat_count() -> usize {
    u32_at(DATA, 12) as usize
}
pub fn emoji_count() -> u32 {
    u32_at(DATA, 16)
}
fn item_off(i: u32) -> usize {
    HEADER + i as usize * ITEM
}
fn kw_table() -> usize {
    HEADER + item_count() as usize * ITEM
}
fn cat_table() -> usize {
    kw_table() + kw_count() * 8
}

pub fn is_emoji(i: u32) -> bool {
    i < emoji_count()
}
pub fn text(i: u32) -> &'static str {
    let o = item_off(i);
    string(u32_at(DATA, o), (u16_at(DATA, o + 4) & 0x7FFF) as usize)
}
pub fn is_skin_variant(i: u32) -> bool {
    u16_at(DATA, item_off(i) + 4) & 0x8000 != 0
}
fn kw_id(i: u32) -> usize {
    u16_at(DATA, item_off(i) + 6) as usize
}
fn kw_str(k: usize) -> &'static str {
    let o = kw_table() + k * 8;
    string(u32_at(DATA, o), u32_at(DATA, o + 4) as usize)
}

/// Deflated SVG bytes of an emoji.
pub fn svg_deflated(i: u32) -> Option<&'static [u8]> {
    let o = item_off(i);
    let (off, len) = (u32_at(DATA, o + 8), u32_at(DATA, o + 12));
    (off != u32::MAX).then(|| &SVGS[off as usize..(off + len) as usize])
}

/// Category `c` of a mode (`kao` = kaomoji): (item range, display name).
pub fn category(kao: bool, c: usize) -> (std::ops::Range<u32>, &'static str) {
    let idx = if kao { emoji_cat_count() + c } else { c };
    let o = cat_table() + idx * 16;
    let r = u32_at(DATA, o)..u32_at(DATA, o + 4);
    (r, string(u32_at(DATA, o + 8), u32_at(DATA, o + 12) as usize))
}

/// Items of a category in display order (skin-tone variants are only shown in search).
pub fn category_items(kao: bool, c: usize) -> Vec<u32> {
    category(kao, c).0.filter(|&i| !is_skin_variant(i)).collect()
}

/// The longest emoji that `s` starts with, and its length in bytes. Only fully-qualified
/// emojis are in the table, so text symbols like ♡ or a bare ☀ don't match.
pub fn emoji_prefix(s: &str) -> Option<(u32, usize)> {
    let b = s.as_bytes();
    if b.first()?.is_ascii() && !matches!(b.get(1..4), Some([0xEF, 0xB8, 0x8F] | [0xE2, 0x83, 0xA3])) {
        return None; // plain ASCII, not a keycap like #️⃣
    }
    let base = cat_table() + (emoji_cat_count() + kao_cat_count()) * 16;
    let id = |k: usize| u16_at(DATA, base + k * 2) as u32;
    (1..=s.len().min(64)).rev().filter(|&e| s.is_char_boundary(e)).find_map(|e| {
        let (mut lo, mut hi) = (0, emoji_count() as usize);
        while lo < hi {
            let m = (lo + hi) / 2;
            match text(id(m)).cmp(&s[..e]) {
                std::cmp::Ordering::Less => lo = m + 1,
                std::cmp::Ordering::Greater => hi = m,
                std::cmp::Ordering::Equal => return Some((id(m), e)),
            }
        }
        None
    })
}

pub fn find_text(s: &str) -> Option<u32> {
    (0..item_count()).find(|&i| text(i) == s)
}

/// Every query word must appear in the keyword. Results: best matches first,
/// emojis before kaomojis, original order otherwise.
pub fn search(query: &str) -> Vec<u32> {
    let q = query.to_lowercase();
    let words: Vec<&str> = q.split_whitespace().collect();
    if words.is_empty() {
        return Vec::new();
    }
    let wants_skin = q.contains("skin") || q.contains("tone");
    // Score per keyword: 0 = no match, 1 = substring, 2 = a word starts with the query word,
    // 3 = keyword starts with the first query word.
    let scores: Vec<u8> = (0..kw_count())
        .map(|k| {
            let kw = kw_str(k);
            let mut best = 3u8;
            for w in &words {
                let s = if kw.starts_with(w) {
                    3
                } else if kw.split(|c: char| !c.is_alphanumeric()).any(|p| p.starts_with(w)) {
                    2
                } else if kw.contains(w) {
                    1
                } else {
                    return 0;
                };
                best = best.min(s);
            }
            best
        })
        .collect();
    let mut hits: Vec<(u8, u32)> = (0..item_count())
        .filter(|&i| wants_skin || !is_skin_variant(i))
        .filter_map(|i| {
            let s = scores[kw_id(i)];
            (s > 0).then_some((s, i))
        })
        .collect();
    hits.sort_by_key(|&(s, i)| (!is_emoji(i), std::cmp::Reverse(s), i));
    hits.into_iter().map(|(_, i)| i).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts() {
        assert!(emoji_count() > 3500, "emojis: {}", emoji_count());
        assert!(item_count() - emoji_count() > 50_000);
        assert_eq!(emoji_cat_count(), 9);
        assert_eq!(kao_cat_count(), 15);
        assert!(category(true, 3).1.starts_with("Crying"));
    }

    #[test]
    fn search_cry() {
        let r: Vec<&str> = search("cry").into_iter().map(text).collect();
        assert!(r.contains(&"😢"), "{:?}", &r[..10.min(r.len())]);
        assert!(r.contains(&"(╥﹏╥)"));
        assert!(r.iter().position(|t| *t == "😢") < r.iter().position(|t| *t == "(╥﹏╥)"));
    }

    #[test]
    fn emoji_in_text() {
        let t = |s: &str| emoji_prefix(s).map(|(i, n)| (text(i), n));
        assert_eq!(t("🎂)"), Some(("🎂", 4)));
        assert_eq!(t("👨‍👩‍👧x"), Some(("👨‍👩‍👧", 18)));
        assert_eq!(t("#️⃣"), Some(("#️⃣", 7)));
        assert_eq!(t("♡"), None);
        assert_eq!(t("☀(・∀・)"), None);
        assert_eq!(t("a🎂"), None);
    }

    #[test]
    fn svg_decodes() {
        let i = find_text("😀").unwrap();
        let svg = miniz_oxide::inflate::decompress_to_vec(svg_deflated(i).unwrap()).unwrap();
        assert!(svg.starts_with(b"<svg"));
    }
}
