//! Turns emoji-test.txt, kaomojis-msime.txt and assets/twemoji/*.svg into three
//! compact blobs that are embedded in the binary (see src/data.rs for the layout).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

const EMOJI_CATS: &[(&str, &str)] = &[
    ("Smileys & Emotion", "Smileys"),
    ("People & Body", "People/Body"),
    ("Animals & Nature", "Nature"),
    ("Food & Drink", "Food"),
    ("Travel & Places", "Travel"),
    ("Activities", "Activities"),
    ("Objects", "Objects"),
    ("Symbols", "Symbols"),
    ("Flags", "Flags"),
];

/// (word that sorts a kaomoji into the tab, label shown in the sidebar). The first keyword
/// word picks the tab; failing that, any keyword word (plurals too), else "Other".
const KAO_CATS: &[(&str, &str)] = &[
    ("happy", "Happy    (◠‿◠)"),
    ("cute", "Cute  (◍•ᴗ•◍)"),
    ("love", "Love  ♡˙ᵕ˙♡"),
    ("crying", "Crying  (ﾟ´Д`ﾟ)ﾟ"),
    ("sad", "Sad    (。•́︿•̀。)"),
    ("angry", "Angry    (╯°□°）╯┻┻"),
    ("surprised", "Surprised    (･口･)"),
    ("sleepy", "Sleepy  ꒰ ˘͈ᵕ˘͈꒱ᶻᶻ"),
    ("greeting", "Greeting  (｡･∀･)ﾉﾞ"),
    ("animal", "Animal  (・ω・)"),
    ("food", "Food  (ノ≧ڡ≦)"),
    ("seasonal", "Seasonal  ☀(・∀・)☀"),
    ("activity", "Activity  ♪◖ᑊ ˘ω˘ ᑊ◗♪"),
    ("decorated", "Decorated    (ㆁᴗㆁ✿)"),
];

struct Item {
    text: String,
    kw: String,
    cat: usize,
    skin: bool,
    svg: Option<Vec<u8>>,
}

fn main() {
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let emoji_path = root.join("emoji-test.txt");
    let kao_path = root.join("kaomojis-msime.txt");
    let svg_dir = root.join("assets/twemoji");
    for p in [&emoji_path, &kao_path, &svg_dir] {
        println!("cargo:rerun-if-changed={}", p.display());
    }

    let emojis = parse_emojis(&emoji_path, &svg_dir);
    let kaomojis = parse_kaomojis(&kao_path);

    let mut strings: Vec<u8> = Vec::new();
    let mut svgs: Vec<u8> = Vec::new();
    let mut kw_ids: HashMap<String, u32> = HashMap::new();
    let mut kws: Vec<(u32, u32)> = Vec::new();
    let mut items: Vec<u8> = Vec::new();
    let mut cats: Vec<(u32, u32, u32, u32)> = Vec::new();
    let mut by_text: Vec<(&str, u32)> = Vec::new();

    let push_str = |s: &str, strings: &mut Vec<u8>| -> (u32, u32) {
        let off = strings.len() as u32;
        strings.extend_from_slice(s.as_bytes());
        (off, s.len() as u32)
    };

    let n_ecat = EMOJI_CATS.len();
    let n_kcat = KAO_CATS.len() + 1; // + "Other"
    let mut idx = 0u32;
    for (group_items, names) in [
        (
            &emojis,
            EMOJI_CATS
                .iter()
                .map(|c| c.1.to_string())
                .collect::<Vec<_>>(),
        ),
        (
            &kaomojis,
            KAO_CATS
                .iter()
                .map(|c| c.1.to_string())
                .chain(["Other".to_string()])
                .collect(),
        ),
    ] {
        for (ci, name) in names.iter().enumerate() {
            let start = idx;
            for it in group_items.iter().filter(|it| it.cat == ci) {
                if (idx as usize) < emojis.len() {
                    by_text.push((&it.text, idx));
                }
                let (toff, tlen) = push_str(&it.text, &mut strings);
                assert!(tlen < 0x8000, "item text too long");
                let kw = *kw_ids.entry(it.kw.clone()).or_insert_with(|| {
                    let (o, l) = push_str(&it.kw, &mut strings);
                    kws.push((o, l));
                    (kws.len() - 1) as u32
                });
                let (soff, slen) = match &it.svg {
                    Some(svg) => {
                        let c = miniz_oxide::deflate::compress_to_vec(svg, 10);
                        let o = svgs.len() as u32;
                        svgs.extend_from_slice(&c);
                        (o, c.len() as u32)
                    }
                    None => (u32::MAX, 0),
                };
                items.extend_from_slice(&toff.to_le_bytes());
                items.extend_from_slice(
                    &((tlen as u16) | if it.skin { 0x8000 } else { 0 }).to_le_bytes(),
                );
                items.extend_from_slice(&(kw as u16).to_le_bytes());
                items.extend_from_slice(&soff.to_le_bytes());
                items.extend_from_slice(&slen.to_le_bytes());
                idx += 1;
            }
            let (no, nl) = push_str(name, &mut strings);
            cats.push((start, idx, no, nl));
        }
    }
    assert!(kws.len() < 0x10000);

    let mut data = Vec::new();
    for v in [
        idx,
        kws.len() as u32,
        n_ecat as u32,
        n_kcat as u32,
        emojis.len() as u32,
    ] {
        data.extend_from_slice(&v.to_le_bytes());
    }
    data.extend_from_slice(&items);
    for (o, l) in &kws {
        data.extend_from_slice(&o.to_le_bytes());
        data.extend_from_slice(&l.to_le_bytes());
    }
    for (a, b, c, d) in &cats {
        for v in [a, b, c, d] {
            data.extend_from_slice(&v.to_le_bytes());
        }
    }
    // Emoji ids sorted by text, to find emojis inside kaomoji/UI text by binary search.
    by_text.sort();
    for (_, i) in by_text {
        data.extend_from_slice(&(i as u16).to_le_bytes());
    }

    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    fs::write(out.join("data.bin"), data).unwrap();
    fs::write(out.join("strings.bin"), strings).unwrap();
    fs::write(out.join("svgs.bin"), svgs).unwrap();
    // Every distinct kaomoji char, so startup can pick their fonts without reading all kaomojis.
    let chars: std::collections::BTreeSet<char> = kaomojis.iter().flat_map(|k| k.text.chars()).collect();
    fs::write(out.join("kao_chars.txt"), chars.into_iter().collect::<String>()).unwrap();
}

fn parse_emojis(path: &Path, svg_dir: &Path) -> Vec<Item> {
    let text = fs::read_to_string(path).expect("emoji-test.txt");
    let mut out = Vec::new();
    let (mut cat, mut sub) = (None, String::new());
    let mut missing = 0;
    for line in text.lines() {
        if let Some(g) = line.strip_prefix("# group: ") {
            cat = EMOJI_CATS.iter().position(|c| c.0 == g.trim());
            continue;
        }
        if let Some(s) = line.strip_prefix("# subgroup: ") {
            sub = s.trim().replace('-', " ");
            continue;
        }
        let Some(cat) = cat else { continue };
        if line.starts_with('#') || !line.contains("; fully-qualified") {
            continue;
        }
        let cps: Vec<u32> = line
            .split(';')
            .next()
            .unwrap()
            .split_whitespace()
            .map(|h| u32::from_str_radix(h, 16).unwrap())
            .collect();
        let comment = line.split_once('#').unwrap().1.trim();
        // "😀 E1.0 grinning face" -> "grinning face"
        let name = comment.splitn(3, ' ').nth(2).unwrap_or("").to_lowercase();
        let text: String = cps.iter().filter_map(|&c| char::from_u32(c)).collect();
        let svg = find_svg(svg_dir, &cps);
        if svg.is_none() {
            missing += 1;
            continue;
        }
        out.push(Item {
            text,
            kw: format!("{name} {sub}"),
            cat,
            skin: cps.iter().any(|c| (0x1F3FB..=0x1F3FF).contains(c)),
            svg,
        });
    }
    if missing > 0 {
        println!("cargo:warning={missing} emojis have no Twemoji SVG and were skipped");
    }
    out
}

fn find_svg(dir: &Path, cps: &[u32]) -> Option<Vec<u8>> {
    let name = |cps: &mut dyn Iterator<Item = &u32>| {
        cps.map(|c| format!("{c:x}")).collect::<Vec<_>>().join("-")
    };
    let full = name(&mut cps.iter());
    let no_fe0f = name(&mut cps.iter().filter(|&&c| c != 0xFE0F));
    [full, no_fe0f]
        .iter()
        .find_map(|n| fs::read(dir.join(format!("{n}.svg"))).ok())
        .map(|s| minify_svg(&s))
}

/// Twemoji SVGs are already compact; just drop newlines and indentation.
fn minify_svg(s: &[u8]) -> Vec<u8> {
    let s = String::from_utf8_lossy(s);
    s.lines()
        .map(str::trim)
        .collect::<Vec<_>>()
        .join(" ")
        .into_bytes()
}

fn has_word(text: &str, word: &str) -> bool {
    text.split(|c: char| !c.is_alphanumeric())
        .any(|w| w == word || w.strip_suffix('s') == Some(word))
}

fn parse_kaomojis(path: &Path) -> Vec<Item> {
    let text = fs::read_to_string(path).expect("kaomojis-msime.txt");
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for line in text.lines() {
        if line.starts_with('!') || line.trim().is_empty() {
            continue;
        }
        let mut parts = line.split('\t');
        let (Some(kw), Some(kao)) = (parts.next(), parts.next()) else {
            continue;
        };
        let (kw, kao) = (kw.trim().to_lowercase(), kao.trim());
        if kao.is_empty() || !seen.insert(kao.to_string()) {
            continue;
        }
        let first = kw.split_whitespace().next().unwrap_or("");
        let cat = KAO_CATS
            .iter()
            .position(|c| c.0 == first)
            .or_else(|| KAO_CATS.iter().position(|c| has_word(&kw, c.0)))
            .unwrap_or(KAO_CATS.len());
        out.push(Item {
            text: kao.to_string(),
            kw,
            cat,
            skin: false,
            svg: None,
        });
    }
    out
}
