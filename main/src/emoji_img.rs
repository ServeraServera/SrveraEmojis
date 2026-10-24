//! Emoji images: inflate the embedded SVG, render it with resvg at the exact
//! on-screen size and keep a small cache of the results.

use crate::data;
use resvg::tiny_skia::{Pixmap, Transform};
use std::collections::HashMap;

const MAX_CACHED: usize = 160;

#[derive(Default)]
pub struct EmojiCache {
    map: HashMap<(u32, u32), Pixmap>,
}

impl EmojiCache {
    pub fn clear(&mut self) {
        self.map = HashMap::new();
    }

    /// The emoji `item` rendered as a `px`×`px` image.
    pub fn get(&mut self, item: u32, px: u32) -> Option<&Pixmap> {
        let key = (item, px);
        if !self.map.contains_key(&key) {
            if self.map.len() >= MAX_CACHED {
                self.map.clear(); // simple and good enough: the visible set is rebuilt in one frame
            }
            let pm = render(item, px)?;
            self.map.insert(key, pm);
        }
        self.map.get(&key)
    }
}

fn render(item: u32, px: u32) -> Option<Pixmap> {
    let svg = miniz_oxide::inflate::decompress_to_vec(data::svg_deflated(item)?).ok()?;
    let tree = resvg::usvg::Tree::from_data(&svg, &resvg::usvg::Options::default()).ok()?;
    let mut pm = Pixmap::new(px, px)?;
    let size = tree.size();
    let s = px as f32 / size.width().max(size.height());
    resvg::render(&tree, Transform::from_scale(s, s), &mut pm.as_mut());
    Some(pm)
}
