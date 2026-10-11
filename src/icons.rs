//! Icon theme lookup (freedesktop Icon Theme spec, simplified) and icon loading.

use crate::config::home_dir;
use crate::desktop::data_dirs;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use tiny_skia::{FilterQuality, Pixmap, PixmapPaint, Transform};

#[cfg(feature = "svg")]
const EXTENSIONS: &[&str] = &["png", "svg"];
#[cfg(not(feature = "svg"))]
const EXTENSIONS: &[&str] = &["png"];

struct ThemeDir {
    sub: String,
    size: u32,
    min: u32,
    max: u32,
    threshold: u32,
    kind: DirKind,
}

#[derive(PartialEq)]
enum DirKind {
    Fixed,
    Scalable,
    Threshold,
}

impl ThemeDir {
    /// Distance between the requested size and this directory (spec: `DirectorySizeDistance`).
    fn distance(&self, size: u32) -> u32 {
        match self.kind {
            DirKind::Fixed => self.size.abs_diff(size),
            DirKind::Scalable if size < self.min => self.min - size,
            DirKind::Scalable if size > self.max => size - self.max,
            DirKind::Threshold if size + self.threshold < self.size => self.min.saturating_sub(size),
            DirKind::Threshold if size > self.size + self.threshold => size.saturating_sub(self.max),
            DirKind::Scalable | DirKind::Threshold => 0,
        }
    }
}

struct Theme {
    roots: Vec<PathBuf>,
    dirs: Vec<ThemeDir>,
}

pub struct IconFinder {
    themes: Vec<Theme>,
    pixmaps: Vec<PathBuf>,
}

fn icon_bases() -> Vec<PathBuf> {
    let mut bases = vec![home_dir().join(".icons")];
    bases.extend(data_dirs().into_iter().map(|d| d.join("icons")));
    bases
}

impl IconFinder {
    /// `theme` is the preferred theme; Adwaita and hicolor are always searched after it.
    pub fn new(theme: &str) -> Self {
        IconFinder::with_bases(theme, icon_bases(), PathBuf::from("/usr/share/pixmaps"))
    }

    /// Like [`IconFinder::new`], searching `bases` (directories that hold themes) and
    /// `pixmaps` instead of the XDG directories.
    fn with_bases(theme: &str, bases: Vec<PathBuf>, pixmaps: PathBuf) -> Self {
        let first = if theme.is_empty() { "Adwaita".to_string() } else { theme.to_string() };

        let mut themes = Vec::new();
        let mut seen = HashSet::new();
        let mut queue = vec![first, "Adwaita".to_string(), "hicolor".to_string()];
        queue.reverse();
        while let Some(name) = queue.pop() {
            if !seen.insert(name.clone()) {
                continue;
            }
            let Some((theme, inherits)) = load_theme(&bases, &name) else {
                continue;
            };
            themes.push(theme);
            // Inherited themes come right after this one, hicolor stays last.
            for parent in inherits.into_iter().rev() {
                if !seen.contains(&parent) {
                    queue.push(parent);
                }
            }
        }
        if let Some(pos) =
            themes.iter().position(|t| t.roots.first().and_then(|r| r.file_name()).is_some_and(|n| n == "hicolor"))
        {
            let hicolor = themes.remove(pos);
            themes.push(hicolor);
        }

        let mut pixmaps = vec![pixmaps];
        pixmaps.extend(bases);
        IconFinder { themes, pixmaps }
    }

    /// Finds the best file for an icon name (or returns the name if it is a path).
    pub fn find(&self, name: &str, size: u32) -> Option<PathBuf> {
        if name.is_empty() {
            return None;
        }
        if name.starts_with('/') {
            let path = PathBuf::from(name);
            return path.is_file().then_some(path);
        }
        let name = ["png", "svg", "xpm"].iter().find_map(|ext| name.strip_suffix(&format!(".{ext}"))).unwrap_or(name);

        for theme in &self.themes {
            let mut best: Option<(u32, PathBuf)> = None;
            for dir in &theme.dirs {
                let dist = dir.distance(size);
                if best.as_ref().is_some_and(|(d, _)| *d <= dist) {
                    continue;
                }
                if let Some(path) = theme_file(&theme.roots, &dir.sub, name) {
                    best = Some((dist, path));
                    if dist == 0 {
                        break;
                    }
                }
            }
            if let Some((_, path)) = best {
                return Some(path);
            }
        }
        self.pixmaps
            .iter()
            .find_map(|base| EXTENSIONS.iter().map(|ext| base.join(format!("{name}.{ext}"))).find(|p| p.is_file()))
    }
}

fn theme_file(roots: &[PathBuf], sub: &str, name: &str) -> Option<PathBuf> {
    for root in roots {
        for ext in EXTENSIONS {
            let path = root.join(sub).join(format!("{name}.{ext}"));
            if path.is_file() {
                return Some(path);
            }
        }
    }
    None
}

/// Reads index.theme of a theme; returns the theme and its parents.
fn load_theme(bases: &[PathBuf], name: &str) -> Option<(Theme, Vec<String>)> {
    let roots: Vec<PathBuf> = bases.iter().map(|b| b.join(name)).filter(|p| p.is_dir()).collect();
    let index = roots.iter().find_map(|r| fs::read_to_string(r.join("index.theme")).ok())?;

    let mut directories = Vec::new();
    let mut inherits = Vec::new();
    let mut sections: Vec<(String, Vec<(String, String)>)> = Vec::new();
    for line in index.lines() {
        let line = line.trim();
        if let Some(section) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            sections.push((section.to_string(), Vec::new()));
        } else if let (Some((key, value)), Some(last)) = (line.split_once('='), sections.last_mut()) {
            last.1.push((key.trim().to_string(), value.trim().to_string()));
        }
    }
    for (section, keys) in &sections {
        if section != "Icon Theme" {
            continue;
        }
        for (key, value) in keys {
            let list = || value.split(',').map(str::trim).filter(|s| !s.is_empty()).map(String::from);
            match key.as_str() {
                "Directories" => directories.extend(list()),
                "Inherits" => inherits.extend(list()),
                _ => {}
            }
        }
    }

    let mut dirs = Vec::new();
    for sub in directories {
        let Some((_, keys)) = sections.iter().find(|(s, _)| *s == sub) else {
            continue;
        };
        let get = |k: &str| keys.iter().find(|(key, _)| key == k).map(|(_, v)| v.as_str());
        let num = |k: &str| get(k).and_then(|v| v.parse::<u32>().ok());
        if num("Scale").unwrap_or(1) != 1 {
            continue;
        }
        let Some(size) = num("Size") else { continue };
        let kind = match get("Type") {
            Some("Fixed") => DirKind::Fixed,
            Some("Scalable") => DirKind::Scalable,
            _ => DirKind::Threshold,
        };
        dirs.push(ThemeDir {
            min: num("MinSize").unwrap_or(size),
            max: num("MaxSize").unwrap_or(size),
            threshold: num("Threshold").unwrap_or(2),
            sub,
            size,
            kind,
        });
    }
    Some((Theme { roots, dirs }, inherits))
}

/// Loads an icon file and scales it to fit a `size`×`size` square.
/// Returns None for unreadable or fully transparent icons (e.g. SVGs made of text only).
pub fn load_icon(path: &Path, size: u32) -> Option<Pixmap> {
    let data = fs::read(path).ok()?;
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase());
    let icon = if ext.as_deref() == Some("svg") {
        render_svg(&data, size)?
    } else {
        fit(&Pixmap::decode_png(&data).ok()?, size)
    };
    icon.pixels().iter().any(|p| p.alpha() > 0).then_some(icon)
}

#[cfg(feature = "svg")]
fn render_svg(data: &[u8], size: u32) -> Option<Pixmap> {
    use resvg::{tiny_skia as rts, usvg};
    let tree = usvg::Tree::from_data(data, &usvg::Options::default()).ok()?;
    let (w, h) = (tree.size().width(), tree.size().height());
    let scale = size as f32 / w.max(h);
    let (dx, dy) = ((size as f32 - w * scale) / 2.0, (size as f32 - h * scale) / 2.0);
    let mut pm = rts::Pixmap::new(size, size)?;
    resvg::render(&tree, rts::Transform::from_scale(scale, scale).post_translate(dx, dy), &mut pm.as_mut());
    Pixmap::from_vec(pm.take(), tiny_skia::IntSize::from_wh(size, size)?)
}

#[cfg(not(feature = "svg"))]
fn render_svg(_: &[u8], _: u32) -> Option<Pixmap> {
    None
}

/// Scales an image (premultiplied RGBA) to fit a square, keeping its aspect ratio.
pub fn fit(image: &Pixmap, size: u32) -> Pixmap {
    let (w, h) = (image.width() as f32, image.height() as f32);
    let scale = size as f32 / w.max(h);
    let mut out = Pixmap::new(size, size).expect("icon size is non-zero");
    let paint = PixmapPaint {
        quality: if scale < 1.0 { FilterQuality::Bicubic } else { FilterQuality::Bilinear },
        ..PixmapPaint::default()
    };
    let (dx, dy) = ((size as f32 - w * scale) / 2.0, (size as f32 - h * scale) / 2.0);
    out.draw_pixmap(0, 0, image.as_ref(), &paint, Transform::from_scale(scale, scale).post_translate(dx, dy), None);
    out
}

/// Converts _`NET_WM_ICON` data (several ARGB images) to the best fitting pixmap.
pub fn from_net_wm_icon(data: &[u32], size: u32) -> Option<Pixmap> {
    let mut best: Option<(u32, u32, &[u32])> = None;
    let mut rest = data;
    while rest.len() >= 2 {
        let (w, h) = (rest[0], rest[1]);
        let len = (w as usize).checked_mul(h as usize)?;
        if w == 0 || h == 0 || rest.len() < 2 + len {
            break;
        }
        let pixels = &rest[2..2 + len];
        let better = match best {
            None => true,
            // Prefer the smallest image that is at least `size`, else the largest.
            Some((bw, _, _)) => (w >= size && (bw < size || w < bw)) || (bw < size && w > bw),
        };
        if better {
            best = Some((w, h, pixels));
        }
        rest = &rest[2 + len..];
    }
    let (w, h, pixels) = best?;
    let mut rgba = Vec::with_capacity(pixels.len() * 4);
    for &argb in pixels {
        let a = (argb >> 24) & 0xff;
        let premul = |c: u32| ((c & 0xff) * a / 255) as u8;
        rgba.extend_from_slice(&[premul(argb >> 16), premul(argb >> 8), premul(argb), a as u8]);
    }
    let image = Pixmap::from_vec(rgba, tiny_skia::IntSize::from_wh(w, h)?)?;
    Some(fit(&image, size))
}

/// Reads Net/IconThemeName from an XSETTINGS blob (published by xfsettingsd).
pub fn xsettings_icon_theme(data: &[u8]) -> Option<String> {
    let big_endian = *data.first()? == 1;
    let u16_at = |i: usize| -> Option<u16> {
        let b: [u8; 2] = data.get(i..i + 2)?.try_into().ok()?;
        Some(if big_endian { u16::from_be_bytes(b) } else { u16::from_le_bytes(b) })
    };
    let u32_at = |i: usize| -> Option<u32> {
        let b: [u8; 4] = data.get(i..i + 4)?.try_into().ok()?;
        Some(if big_endian { u32::from_be_bytes(b) } else { u32::from_le_bytes(b) })
    };
    let pad4 = |n: usize| (n + 3) & !3;
    let count = u32_at(8)?;
    let mut pos = 12;
    for _ in 0..count {
        let kind = *data.get(pos)?;
        let name_len = u16_at(pos + 2)? as usize;
        let name = data.get(pos + 4..pos + 4 + name_len)?;
        pos += 4 + pad4(name_len) + 4; // header, name, last-change serial
        match kind {
            0 => pos += 4,
            1 => {
                let len = u32_at(pos)? as usize;
                let value = data.get(pos + 4..pos + 4 + len)?;
                if name == b"Net/IconThemeName" {
                    return String::from_utf8(value.to_vec()).ok().filter(|v| !v.is_empty());
                }
                pos += 4 + pad4(len);
            }
            2 => pos += 8,
            _ => return None,
        }
    }
    None
}

/// Fallback when xfsettingsd is not running: XFCE's saved xsettings channel.
pub fn xfconf_icon_theme() -> Option<String> {
    xfconf_icon_theme_in(&home_dir().join(".config/xfce4/xfconf/xfce-perchannel-xml/xsettings.xml"))
}

fn xfconf_icon_theme_in(path: &Path) -> Option<String> {
    let xml = fs::read_to_string(path).ok()?;
    let line = xml.lines().find(|l| l.contains("name=\"IconThemeName\""))?;
    let value = line.split("value=\"").nth(1)?.split('"').next()?;
    (!value.is_empty()).then(|| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_xsettings() {
        let mut d = vec![0, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0];
        // integer setting "Net/A" = 5
        d.extend_from_slice(&[0, 0, 5, 0]);
        d.extend_from_slice(b"Net/A\0\0\0");
        d.extend_from_slice(&[0; 4]);
        d.extend_from_slice(&5u32.to_le_bytes());
        // string setting
        let name = b"Net/IconThemeName";
        d.extend_from_slice(&[1, 0, name.len() as u8, 0]);
        d.extend_from_slice(name);
        d.extend_from_slice(&[0; 3]);
        d.extend_from_slice(&[0; 4]);
        d.extend_from_slice(&8u32.to_le_bytes());
        d.extend_from_slice(b"elementa");
        assert_eq!(xsettings_icon_theme(&d).as_deref(), Some("elementa"));
        // Truncated data and unknown setting types are refused, not read past their end.
        assert_eq!(xsettings_icon_theme(&d[..d.len() - 4]), None);
        assert_eq!(xsettings_icon_theme(&[0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 9, 0, 1, 0]), None);
        assert_eq!(xsettings_icon_theme(&[]), None);
    }

    /// A 16x16 opaque PNG.
    fn png() -> Vec<u8> {
        let mut pm = Pixmap::new(16, 16).unwrap();
        pm.fill(tiny_skia::Color::from_rgba8(200, 30, 30, 255));
        pm.encode_png().unwrap()
    }

    /// Two themes under `base`: "Mine" (16 and 48 px, inherits "Parent") and "Parent"
    /// (scalable), plus hicolor.
    fn themes(base: &Path) {
        let write = |rel: &str, data: &[u8]| {
            let path = base.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, data).unwrap();
        };
        write(
            "Mine/index.theme",
            b"[Icon Theme]\nName=Mine\nInherits=Parent\nDirectories=16x16/apps,48x48/apps,48x48@2/apps\n\
              [16x16/apps]\nSize=16\nType=Fixed\n[48x48/apps]\nSize=48\nType=Threshold\n\
              [48x48@2/apps]\nSize=48\nScale=2\n",
        );
        write("Mine/16x16/apps/small.png", &png());
        write("Mine/48x48/apps/small.png", &png());
        write("Mine/16x16/apps/only16.png", &png());
        write(
            "Parent/index.theme",
            b"[Icon Theme]\nName=Parent\nDirectories=scalable/apps\n[scalable/apps]\nSize=64\nMinSize=8\nMaxSize=512\nType=Scalable\n",
        );
        write("Parent/scalable/apps/vector.svg", br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10" fill="blue"/></svg>"#);
        write("hicolor/index.theme", b"[Icon Theme]\nName=Hicolor\nDirectories=48x48/apps\n[48x48/apps]\nSize=48\n");
        write("hicolor/48x48/apps/fallback.png", &png());
    }

    #[test]
    fn finds_icons_by_size_theme_and_fallback() {
        let dir = tempfile::tempdir().unwrap();
        themes(dir.path());
        let pixmaps = dir.path().join("pixmaps");
        fs::create_dir_all(&pixmaps).unwrap();
        fs::write(pixmaps.join("legacy.png"), png()).unwrap();
        let finder = IconFinder::with_bases("Mine", vec![dir.path().to_path_buf()], pixmaps.clone());
        let rel = |p: Option<PathBuf>| p.map(|p| p.strip_prefix(dir.path()).unwrap().to_string_lossy().into_owned());
        // The closest size wins; a name with an extension is the same icon.
        assert_eq!(rel(finder.find("small", 48)).as_deref(), Some("Mine/48x48/apps/small.png"));
        assert_eq!(rel(finder.find("small.png", 16)).as_deref(), Some("Mine/16x16/apps/small.png"));
        assert_eq!(rel(finder.find("only16", 48)).as_deref(), Some("Mine/16x16/apps/only16.png"));
        // Inherited theme, then hicolor, then the pixmaps directory.
        #[cfg(feature = "svg")]
        assert_eq!(rel(finder.find("vector", 48)).as_deref(), Some("Parent/scalable/apps/vector.svg"));
        #[cfg(not(feature = "svg"))]
        assert_eq!(finder.find("vector", 48), None, "SVG icons are skipped without the svg feature");
        assert_eq!(rel(finder.find("fallback", 48)).as_deref(), Some("hicolor/48x48/apps/fallback.png"));
        assert_eq!(rel(finder.find("legacy", 48)).as_deref(), Some("pixmaps/legacy.png"));
        assert_eq!(finder.find("missing", 48), None);
        assert_eq!(finder.find("", 48), None);
        // An absolute path is used as it is, when it exists.
        let abs = pixmaps.join("legacy.png");
        assert_eq!(finder.find(&abs.to_string_lossy(), 48), Some(abs));
        assert_eq!(finder.find("/no/such/icon.png", 48), None);
        assert_eq!(finder.themes.last().and_then(|t| t.roots.first()).map(|r| r.ends_with("hicolor")), Some(true));
    }

    #[test]
    fn theme_directories_measure_their_distance() {
        let dir = |kind, size, min, max| ThemeDir { sub: String::new(), size, min, max, threshold: 2, kind };
        assert_eq!(dir(DirKind::Fixed, 48, 48, 48).distance(32), 16);
        assert_eq!(dir(DirKind::Scalable, 64, 16, 256).distance(8), 8);
        assert_eq!(dir(DirKind::Scalable, 64, 16, 256).distance(300), 44);
        assert_eq!(dir(DirKind::Scalable, 64, 16, 256).distance(48), 0);
        assert_eq!(dir(DirKind::Threshold, 48, 48, 48).distance(47), 0);
        assert_eq!(dir(DirKind::Threshold, 48, 48, 48).distance(32), 16);
        assert_eq!(dir(DirKind::Threshold, 48, 48, 48).distance(64), 16);
    }

    #[test]
    fn loads_png_and_svg_icons_at_the_requested_size() {
        let dir = tempfile::tempdir().unwrap();
        themes(dir.path());
        let png_icon = load_icon(&dir.path().join("Mine/16x16/apps/small.png"), 32).unwrap();
        assert_eq!((png_icon.width(), png_icon.height()), (32, 32));
        assert_eq!(png_icon.pixel(16, 16).map(tiny_skia::PremultipliedColorU8::red), Some(200));
        #[cfg(feature = "svg")]
        {
            let svg = load_icon(&dir.path().join("Parent/scalable/apps/vector.svg"), 24).unwrap();
            assert_eq!(svg.pixel(12, 12).map(|p| (p.blue(), p.alpha())), Some((255, 255)));
        }
        // Unreadable, broken or fully transparent icons give nothing.
        let empty = dir.path().join("empty.svg");
        fs::write(&empty, r#"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4"></svg>"#).unwrap();
        assert!(load_icon(&empty, 24).is_none());
        fs::write(dir.path().join("broken.png"), b"not a png").unwrap();
        assert!(load_icon(&dir.path().join("broken.png"), 24).is_none());
        assert!(load_icon(&dir.path().join("missing.png"), 24).is_none());
    }

    #[test]
    fn picks_the_closest_window_icon() {
        // A 2x2 red image and a 4x4 blue one.
        let mut data = vec![2, 2];
        data.extend([0xffff_0000; 4]);
        data.extend([4, 4]);
        data.extend([0xff00_00ff; 16]);
        let icon = from_net_wm_icon(&data, 4).unwrap();
        assert_eq!(icon.pixel(1, 1).map(|p| (p.red(), p.blue())), Some((0, 255)));
        assert!(from_net_wm_icon(&[16, 16, 0xffff_ffff], 16).is_none(), "truncated");
        assert!(from_net_wm_icon(&[], 16).is_none());
    }

    #[test]
    fn reads_the_xfce_icon_theme() {
        let dir = tempfile::tempdir().unwrap();
        let xml = dir.path().join("xsettings.xml");
        fs::write(&xml, "<channel>\n  <property name=\"IconThemeName\" type=\"string\" value=\"elementary-xfce-dark\"/>\n</channel>\n").unwrap();
        assert_eq!(xfconf_icon_theme_in(&xml).as_deref(), Some("elementary-xfce-dark"));
        fs::write(&xml, "<property name=\"IconThemeName\" type=\"string\" value=\"\"/>").unwrap();
        assert_eq!(xfconf_icon_theme_in(&xml), None);
        assert_eq!(xfconf_icon_theme_in(&dir.path().join("missing.xml")), None);
    }
}
