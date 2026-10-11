//! Tiny text renderer for tooltips and menus: glyph outlines from skrifa, filled with
//! tiny-skia. Only the font file is kept in memory.

use crate::config::Color;
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::{FontRef, GlyphId, MetadataProvider};
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Transform};

const FALLBACK_FONTS: &[&str] = &[
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
    "/usr/share/fonts/opentype/noto/NotoSans-Regular.ttf",
    "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
    "/usr/share/fonts/truetype/liberation2/LiberationSans-Regular.ttf",
    "/usr/share/fonts/truetype/freefont/FreeSans.ttf",
];

pub struct Text {
    data: Vec<u8>,
    px: f32,
    /// Ascender and line height, in font units.
    ascender: f32,
    height: f32,
    units_per_em: f32,
}

impl Text {
    pub fn load(configured: &str, px: f32) -> Option<Self> {
        let fontconfig = fontconfig_sans();
        let candidates = std::iter::once(configured)
            .chain(fontconfig.as_deref())
            .filter(|p| !p.is_empty())
            .chain(FALLBACK_FONTS.iter().copied());
        for path in candidates {
            let Ok(data) = std::fs::read(path) else {
                continue;
            };
            let Ok(font) = FontRef::from_index(&data, 0) else {
                eprintln!("dockling: cannot parse font {path}");
                continue;
            };
            let metrics = font.metrics(Size::unscaled(), LocationRef::default());
            let (ascender, units_per_em) = (metrics.ascent, f32::from(metrics.units_per_em));
            // The descent is below the baseline, so it is negative.
            let height = ascender - metrics.descent + metrics.leading;
            if units_per_em <= 0.0 {
                eprintln!("dockling: cannot use font {path}: no units per em");
                continue;
            }
            return Some(Text { data, px, ascender, height, units_per_em });
        }
        eprintln!("dockling: no usable font found, tooltips and menus are disabled (set `font` in the config)");
        None
    }

    /// The font, parsed again from its bytes (cheap: nothing is copied). It was checked when
    /// loaded, so this only fails if the bytes were changed since, which never happens.
    fn font(&self) -> Option<FontRef<'_>> {
        FontRef::from_index(&self.data, 0).ok()
    }

    pub fn line_height(&self) -> u32 {
        (self.height * self.px / self.units_per_em).ceil() as u32
    }

    pub fn width(&self, s: &str) -> u32 {
        self.width_at(s, self.px)
    }

    pub fn width_at(&self, s: &str, px: f32) -> u32 {
        let Some(font) = self.font() else { return 0 };
        let units: f32 = s.chars().map(|c| advance(&font, glyph(&font, c))).sum();
        (units * px / self.units_per_em).ceil() as u32
    }

    /// Draws `s` with its line box starting at (x, y), clipped to `max_width`.
    pub fn draw(&self, pm: &mut Pixmap, x: i32, y: i32, s: &str, color: Color, max_width: u32) {
        self.draw_at(pm, x, y, s, color, max_width, self.px);
    }

    /// Like `draw`, at another pixel size.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_at(&self, pm: &mut Pixmap, x: i32, y: i32, s: &str, color: Color, max_width: u32, px: f32) {
        let Some(font) = self.font() else { return };
        let outlines = font.outline_glyphs();
        let scale = px / self.units_per_em;
        let baseline = y as f32 + self.ascender * scale;
        let limit = max_width as f32;
        let mut pen = 0.0;
        let mut builder = Outline { pb: PathBuilder::new(), scale, x: 0.0, y: baseline };
        for c in s.chars() {
            let id = glyph(&font, c);
            let adv = advance(&font, id) * scale;
            if pen + adv > limit {
                break;
            }
            builder.x = x as f32 + pen;
            if let Some(outline) = outlines.get(id) {
                // A glyph that fails to draw is left out; the others still show.
                let _ = outline.draw(DrawSettings::unhinted(Size::unscaled(), LocationRef::default()), &mut builder);
            }
            pen += adv;
        }
        let mut paint = Paint::default();
        paint.set_color(color.skia());
        paint.anti_alias = true;
        if let Some(path) = builder.pb.finish() {
            pm.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
        }
    }
}

/// The system's default sans-serif font file, as chosen by fontconfig.
fn fontconfig_sans() -> Option<String> {
    let out = std::process::Command::new("fc-match")
        .args(["-f", "%{file}", "sans-serif:style=Regular"])
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    let path = String::from_utf8(out.stdout).ok()?;
    // fontconfig may pick a format ttf-parser cannot read (e.g. Type 1); the fixed list follows.
    let readable = [".ttf", ".otf", ".ttc"].iter().any(|ext| path.to_lowercase().ends_with(ext));
    (out.status.success() && readable).then_some(path)
}

/// The glyph of `c`, else of "?", else the font's "missing glyph" box.
fn glyph(font: &FontRef, c: char) -> GlyphId {
    let charmap = font.charmap();
    charmap.map(c).or_else(|| charmap.map('?')).unwrap_or(GlyphId::NOTDEF)
}

/// The horizontal advance of a glyph, in font units.
fn advance(font: &FontRef, id: GlyphId) -> f32 {
    font.glyph_metrics(Size::unscaled(), LocationRef::default()).advance_width(id).unwrap_or(0.0)
}

/// Converts font-unit outlines (y up) into a pixel-space path (y down).
struct Outline {
    pb: PathBuilder,
    scale: f32,
    x: f32,
    y: f32,
}

impl Outline {
    fn p(&self, x: f32, y: f32) -> (f32, f32) {
        (self.x + x * self.scale, self.y - y * self.scale)
    }
}

impl OutlinePen for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.p(x, y);
        self.pb.move_to(x, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.p(x, y);
        self.pb.line_to(x, y);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let ((x1, y1), (x, y)) = (self.p(x1, y1), self.p(x, y));
        self.pb.quad_to(x1, y1, x, y);
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let ((x1, y1), (x2, y2), (x, y)) = (self.p(x1, y1), self.p(x2, y2), self.p(x, y));
        self.pb.cubic_to(x1, y1, x2, y2, x, y);
    }

    fn close(&mut self) {
        self.pb.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A font of the build machine (fonts-dejavu-core on Debian and Ubuntu), if any.
    fn text(px: f32) -> Option<Text> {
        let font = FALLBACK_FONTS.iter().find(|p| std::path::Path::new(p).is_file())?;
        Text::load(font, px)
    }

    #[test]
    fn measures_and_draws_text() {
        let Some(text) = text(20.0) else { return };
        let (short, long) = (text.width("Files"), text.width("Files and Folders"));
        assert!(short > 20 && long > 2 * short, "{short} {long}");
        let (w20, w40) = (text.width_at("Files", 20.0), text.width_at("Files", 40.0));
        assert!(w40.abs_diff(2 * w20) <= 1, "twice the size, twice as wide: {w20} {w40}");
        assert!((20..40).contains(&text.line_height()), "{}", text.line_height());

        let mut pm = Pixmap::new(200, 30).unwrap();
        let white = Color { r: 255, g: 255, b: 255, a: 255 };
        text.draw(&mut pm, 0, 0, "Files", white, 200);
        let inked = |pm: &Pixmap, x0: u32, x1: u32| {
            (x0..x1)
                .flat_map(|x| (0..30).map(move |y| (x, y)))
                .filter(|&(x, y)| pm.pixel(x, y).is_some_and(|p| p.alpha() > 0))
                .count()
        };
        assert!(inked(&pm, 0, short) > 50, "the glyphs are filled");
        assert_eq!(inked(&pm, short + 2, 200), 0, "nothing after the text");
        if let Ok(dir) = std::env::var("DOCKLING_TEXT_PNG") {
            let mut sample = Pixmap::new(420, 40).unwrap();
            sample.fill(tiny_skia::Color::from_rgba8(0x30, 0x30, 0x30, 0xff));
            text.draw(&mut sample, 8, 6, "Mousepad · Firefox ESR · Ünïcödé ? 0123", white, 404);
            sample.save_png(format!("{dir}/text.png")).unwrap();
        }
    }

    #[test]
    fn text_is_cut_at_the_width_limit() {
        let Some(text) = text(20.0) else { return };
        let mut pm = Pixmap::new(300, 30).unwrap();
        text.draw(&mut pm, 0, 0, "A long label that does not fit", Color { r: 255, g: 255, b: 255, a: 255 }, 60);
        assert!((60..300).all(|x| (0..30).all(|y| pm.pixel(x, y).is_some_and(|p| p.alpha() == 0))));
    }

    #[test]
    fn unreadable_fonts_are_skipped() {
        let dir = std::env::temp_dir().join(format!("dockling-font-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bad = dir.join("bad.ttf");
        std::fs::write(&bad, b"not a font").unwrap();
        // The configured font is broken: fontconfig's or a fallback font is used instead.
        if FALLBACK_FONTS.iter().any(|p| std::path::Path::new(p).is_file()) {
            assert!(Text::load(&bad.to_string_lossy(), 13.0).is_some());
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
