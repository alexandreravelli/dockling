//! Tiny text renderer for tooltips and menus: glyph outlines from ttf-parser,
//! filled with tiny-skia. Only the font file is kept in memory.

use crate::config::Color;
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Transform};
use ttf_parser::{Face, GlyphId, OutlineBuilder};

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
            let Ok(face) = Face::parse(&data, 0) else {
                eprintln!("dockling: cannot parse font {path}");
                continue;
            };
            let ascender = face.ascender() as f32;
            let height = ascender - face.descender() as f32 + face.line_gap() as f32;
            let units_per_em = face.units_per_em() as f32;
            return Some(Text { data, px, ascender, height, units_per_em });
        }
        eprintln!("dockling: no usable font found, tooltips and menus are disabled (set `font` in the config)");
        None
    }

    fn face(&self) -> Face<'_> {
        Face::parse(&self.data, 0).expect("font was validated on load")
    }

    pub fn line_height(&self) -> u32 {
        (self.height * self.px / self.units_per_em).ceil() as u32
    }

    pub fn width(&self, s: &str) -> u32 {
        self.width_at(s, self.px)
    }

    pub fn width_at(&self, s: &str, px: f32) -> u32 {
        let face = self.face();
        let units: f32 = s.chars().map(|c| advance(&face, glyph(&face, c))).sum();
        (units * px / self.units_per_em).ceil() as u32
    }

    /// Draws `s` with its line box starting at (x, y), clipped to `max_width`.
    pub fn draw(&self, pm: &mut Pixmap, x: i32, y: i32, s: &str, color: Color, max_width: u32) {
        self.draw_at(pm, x, y, s, color, max_width, self.px);
    }

    /// Like `draw`, at another pixel size.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_at(&self, pm: &mut Pixmap, x: i32, y: i32, s: &str, color: Color, max_width: u32, px: f32) {
        let face = self.face();
        let scale = px / self.units_per_em;
        let baseline = y as f32 + self.ascender * scale;
        let limit = max_width as f32;
        let mut pen = 0.0;
        let mut builder = Outline { pb: PathBuilder::new(), scale, x: 0.0, y: baseline };
        for c in s.chars() {
            let id = glyph(&face, c);
            let adv = advance(&face, id) * scale;
            if pen + adv > limit {
                break;
            }
            builder.x = x as f32 + pen;
            face.outline_glyph(id, &mut builder);
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

fn glyph(face: &Face, c: char) -> GlyphId {
    face.glyph_index(c).or_else(|| face.glyph_index('?')).unwrap_or(GlyphId(0))
}

fn advance(face: &Face, id: GlyphId) -> f32 {
    face.glyph_hor_advance(id).unwrap_or(0) as f32
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

impl OutlineBuilder for Outline {
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
