//! Font discovery and per-character face resolution.
//!
//! Faces are found with `fontdb` (no fontconfig) and memory-mapped, so a large
//! CJK collection only costs the pages that are actually read. A character is
//! looked up in the primary face for its style, then the primary regular face
//! (with synthetic bold/italic), then the fallback list, then — once per
//! character — every other installed face. Colour (emoji) faces are passed
//! over for text presentation unless nothing else has the glyph, and
//! preferred for emoji presentation ([`pick`]).

use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use fontdb::{Database, Family, Query, Source, Stretch, Style as FdStyle, Weight};
use memmap2::Mmap;
use swash::{CacheKey, FontRef};

pub use crate::emoji::Presentation;

#[cfg(windows)]
const PRIMARY_DEFAULTS: &[&str] = &["Cascadia Mono", "Consolas", "Lucida Console", "Courier New"];
#[cfg(not(windows))]
const PRIMARY_DEFAULTS: &[&str] =
    &["DejaVu Sans Mono", "Noto Sans Mono", "Liberation Mono", "Ubuntu Mono", "Hack", "Fira Mono"];

/// Tried in order after the primary font. Hangul/CJK first, then symbols.
#[cfg(windows)]
const FALLBACK_DEFAULTS: &[&str] =
    &["Malgun Gothic", "Segoe UI Symbol", "Segoe UI", "Cambria Math", "MS Gothic", "Segoe UI Emoji"];
#[cfg(not(windows))]
const FALLBACK_DEFAULTS: &[&str] = &[
    "Noto Sans Mono CJK KR",
    "Noto Sans CJK KR",
    "NanumGothicCoding",
    "NanumGothic",
    // Debian/Ubuntu fonts-noto-core 20201225 names it "Noto Sans Symbols2".
    "Noto Sans Symbols 2",
    "Noto Sans Symbols2",
    "Noto Sans Symbols",
    "DejaVu Sans",
    "Noto Sans Math",
    "Noto Color Emoji",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
}

impl Style {
    fn index(self) -> usize {
        self.bold as usize | (self.italic as usize) << 1
    }
}

/// A face ready for swash: mapped file data plus the table offset.
pub struct Face {
    data: Arc<Mmap>,
    offset: u32,
    key: CacheKey,
    pub name: String,
    /// Colour glyphs (CBDT, sbix, COLR or SVG table): an emoji font.
    pub color: bool,
}

impl Face {
    pub fn font(&self) -> FontRef<'_> {
        FontRef { data: &self.data[..], offset: self.offset, key: self.key }
    }
}

/// Where a character is drawn from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resolved {
    pub face: usize,
    pub glyph: u16,
    pub synth_bold: bool,
    pub synth_italic: bool,
}

pub struct Fonts {
    db: Database,
    files: HashMap<PathBuf, Arc<Mmap>>,
    faces: Vec<Face>,
    by_id: HashMap<fontdb::ID, usize>,
    /// Faces for regular, bold, italic, bold italic (index = `Style::index`).
    primary: [Option<usize>; 4],
    fallback: Vec<usize>,
    lookup: HashMap<(char, Style, Presentation), Option<Resolved>>,
    /// Faces not yet tried in the last-resort scan.
    unscanned: Option<Vec<fontdb::ID>>,
}

impl Fonts {
    /// Loads the system font list and picks the primary family: `family` if
    /// installed, else the platform defaults, else any monospaced face.
    pub fn load(family: Option<&str>) -> Result<Fonts, String> {
        let mut db = Database::new();
        db.load_system_fonts();
        if let Ok(dir) = std::env::var("CCTERM_FONT_DIR") {
            db.load_fonts_dir(dir);
        }
        Self::from_db(db, family)
    }

    pub fn from_db(db: Database, family: Option<&str>) -> Result<Fonts, String> {
        let mut f = Fonts {
            db,
            files: HashMap::new(),
            faces: vec![],
            by_id: HashMap::new(),
            primary: [None; 4],
            fallback: vec![],
            lookup: HashMap::new(),
            unscanned: None,
        };
        let mut names: Vec<&str> = family.into_iter().collect();
        names.extend_from_slice(PRIMARY_DEFAULTS);
        let mut chosen = None;
        for name in &names {
            if let Some(id) = f.query(name, Weight::NORMAL, FdStyle::Normal) {
                if let Some(idx) = f.open(id) {
                    chosen = Some((name.to_string(), idx));
                    break;
                }
            }
            if Some(*name) == family {
                log::warn!("font family {name:?} not found, using defaults");
            }
        }
        if chosen.is_none() {
            let mono: Vec<_> = f.db.faces().filter(|fi| fi.monospaced).map(|fi| fi.id).collect();
            for id in mono {
                if let Some(idx) = f.open(id) {
                    chosen = Some((f.faces[idx].name.clone(), idx));
                    break;
                }
            }
        }
        if chosen.is_none() {
            let any: Vec<_> = f.db.faces().map(|fi| fi.id).collect();
            for id in any {
                if let Some(idx) = f.open(id) {
                    chosen = Some((f.faces[idx].name.clone(), idx));
                    break;
                }
            }
        }
        let (name, regular) = chosen.ok_or("no usable font found on this system")?;
        f.primary[0] = Some(regular);
        // Real bold/italic faces of the same family, when installed.
        let want = [
            (Weight::BOLD, FdStyle::Normal, 1),
            (Weight::NORMAL, FdStyle::Italic, 2),
            (Weight::BOLD, FdStyle::Italic, 3),
        ];
        for (w, s, slot) in want {
            if let Some(id) = f.query(&name, w, s) {
                let info = f.db.face(id).expect("face");
                let bold_ok = w != Weight::BOLD || info.weight.0 >= 600;
                let italic_ok = s == FdStyle::Normal || info.style != FdStyle::Normal;
                if bold_ok && italic_ok {
                    f.primary[slot] = f.open(id);
                }
            }
        }
        for name in FALLBACK_DEFAULTS {
            if let Some(id) = f.query(name, Weight::NORMAL, FdStyle::Normal) {
                if let Some(idx) = f.open(id) {
                    if !f.fallback.contains(&idx) && Some(idx) != f.primary[0] {
                        f.fallback.push(idx);
                    }
                }
            }
        }
        log::info!(
            "font: primary {:?} (bold {}, italic {}), fallback [{}]",
            f.faces[regular].name,
            f.primary[1].is_some(),
            f.primary[2].is_some(),
            f.fallback.iter().map(|&i| f.faces[i].name.as_str()).collect::<Vec<_>>().join(", ")
        );
        Ok(f)
    }

    fn query(&self, name: &str, weight: Weight, style: FdStyle) -> Option<fontdb::ID> {
        let id = self.db.query(&Query { families: &[Family::Name(name)], weight, stretch: Stretch::Normal, style })?;
        // fontdb matches family names case-insensitively; keep it that way.
        Some(id)
    }

    fn map_file(&mut self, path: &Path) -> Option<Arc<Mmap>> {
        if let Some(m) = self.files.get(path) {
            return Some(m.clone());
        }
        let file = File::open(path).ok()?;
        // SAFETY: font files are not expected to change while mapped; this is
        // the same assumption fontdb's own memmap feature makes.
        let map = Arc::new(unsafe { Mmap::map(&file) }.ok()?);
        self.files.insert(path.to_owned(), map.clone());
        Some(map)
    }

    fn open(&mut self, id: fontdb::ID) -> Option<usize> {
        if let Some(&i) = self.by_id.get(&id) {
            return Some(i);
        }
        let info = self.db.face(id)?;
        let index = info.index;
        let name = info.families.first().map(|(n, _)| n.clone()).unwrap_or_else(|| info.post_script_name.clone());
        let path = match &info.source {
            Source::File(p) => p.clone(),
            _ => return None,
        };
        let data = self.map_file(&path)?;
        let font = FontRef::from_index(&data[..], index as usize)?;
        let (offset, key) = (font.offset, font.key);
        let color = [b"CBDT", b"sbix", b"COLR", b"SVG "].iter().any(|t| font.table(swash::tag_from_bytes(t)).is_some());
        self.faces.push(Face { data, offset, key, name, color });
        let i = self.faces.len() - 1;
        self.by_id.insert(id, i);
        Some(i)
    }

    /// Open faces, mapped files and lookup cache, for the log. Mapped font
    /// files count as file-backed memory, not anonymous.
    pub fn memory(&self) -> String {
        let mapped: usize = self.files.values().map(|m| m.len()).sum();
        format!(
            "{} faces open ({} of {} installed), {} files mapped ({} KB, file-backed), {} lookups cached",
            self.faces.len(),
            self.faces.len(),
            self.db.len(),
            self.files.len(),
            mapped / 1024,
            self.lookup.len()
        )
    }

    pub fn face(&self, i: usize) -> &Face {
        &self.faces[i]
    }

    pub fn primary(&self) -> &Face {
        &self.faces[self.primary[0].expect("primary face")]
    }

    /// Whether `face` is one of the primary family's faces (any style).
    pub fn is_primary(&self, face: usize) -> bool {
        self.primary.contains(&Some(face))
    }

    fn glyph_in(&self, face: usize, c: char) -> u16 {
        self.faces[face].font().charmap().map(c)
    }

    /// The face and glyph for `c` in `style` and presentation `pres`; `None`
    /// when no installed face has the character.
    pub fn resolve(&mut self, c: char, style: Style, pres: Presentation) -> Option<Resolved> {
        if let Some(r) = self.lookup.get(&(c, style, pres)) {
            return *r;
        }
        let r = self.resolve_uncached(c, style, pres);
        self.lookup.insert((c, style, pres), r);
        r
    }

    /// Candidates in priority order — the primary face for the style, the
    /// primary regular face (synthetic bold/italic), the fallback list, then
    /// every other installed face (opened lazily, once per character; a face
    /// that has the glyph joins the fallback list) — chosen by [`pick`].
    fn resolve_uncached(&mut self, c: char, style: Style, pres: Presentation) -> Option<Resolved> {
        let regular = self.primary[0].expect("primary face");
        let mut fixed: Vec<(usize, bool)> = self.primary[style.index()].map(|f| (f, false)).into_iter().collect();
        fixed.push((regular, true));
        fixed.extend(self.fallback.iter().map(|&f| (f, true)));
        let (mut i, mut j) = (0, 0);
        let mut scan: Option<Vec<fontdb::ID>> = None;
        let cands = std::iter::from_fn(|| loop {
            let (face, synth) = if i < fixed.len() {
                i += 1;
                fixed[i - 1]
            } else {
                if self.unscanned.is_none() {
                    let ids = self.db.faces().map(|f| f.id).filter(|id| !self.by_id.contains_key(id)).collect();
                    self.unscanned = Some(ids);
                }
                let ids = scan.get_or_insert_with(|| self.unscanned.clone().unwrap_or_default());
                let id = *ids.get(j)?;
                j += 1;
                let Some(face) = self.open(id) else { continue };
                if self.glyph_in(face, c) != 0 {
                    if !self.fallback.contains(&face) && Some(face) != self.primary[0] {
                        log::debug!("font: {:?} added as fallback for U+{:04X}", self.faces[face].name, c as u32);
                        self.fallback.push(face);
                    }
                    if let Some(list) = self.unscanned.as_mut() {
                        list.retain(|x| *x != id);
                    }
                }
                (face, true)
            };
            let glyph = self.glyph_in(face, c);
            let r = Resolved { face, glyph, synth_bold: synth && style.bold, synth_italic: synth && style.italic };
            return Some((r, self.faces[face].color, glyph != 0));
        });
        pick(pres, cands)
    }
}

/// Chooses from `(item, colour face, has the glyph)` candidates in priority
/// order. Text presentation takes the first monochrome face with the glyph
/// and a colour (emoji) face only when no monochrome face has it — a colour
/// glyph beats a missing one. Emoji presentation is the reverse. Stops at
/// the first preferred hit, so later candidates (the installed-font scan)
/// are only consumed when needed.
pub fn pick<T>(pres: Presentation, cands: impl IntoIterator<Item = (T, bool, bool)>) -> Option<T> {
    let want_color = pres == Presentation::Emoji;
    let mut other = None;
    for (item, color, has) in cands {
        if !has {
            continue;
        }
        if color == want_color {
            return Some(item);
        }
        other.get_or_insert(item);
    }
    other
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Candidates are (name, colour face, has glyph); the iterator records
    /// how many were consumed.
    fn run(pres: Presentation, cands: &[(&'static str, bool, bool)]) -> (Option<&'static str>, usize) {
        let mut n = 0;
        let r = pick(pres, cands.iter().inspect(|_| n += 1).copied());
        (r, n)
    }

    #[test]
    fn fallback_order_by_presentation() {
        use Presentation::{Emoji, Text};
        // ⏸ with Noto Color Emoji before a symbol font: text presentation
        // skips the colour face and takes the monochrome one.
        let cands =
            [("primary", false, false), ("emoji", true, true), ("symbols2", false, true), ("scan", false, true)];
        assert_eq!(run(Text, &cands), (Some("symbols2"), 3));
        // No monochrome face has it: colour beats tofu.
        assert_eq!(
            run(Text, &[("primary", false, false), ("emoji", true, true), ("x", false, false)]).0,
            Some("emoji")
        );
        // Emoji presentation (default or VS16): colour first, even after a
        // monochrome hit, which stays the fallback.
        assert_eq!(run(Emoji, &[("primary", false, true), ("emoji", true, true)]), (Some("emoji"), 2));
        assert_eq!(run(Emoji, &[("symbols2", false, true), ("x", false, false)]).0, Some("symbols2"));
        // Nothing has it.
        assert_eq!(run(Text, &[("a", false, false), ("b", true, false)]).0, None);
        // A monochrome hit in the primary face ends the search at once.
        assert_eq!(run(Text, &[("primary", false, true), ("emoji", true, true)]), (Some("primary"), 1));
    }
}
