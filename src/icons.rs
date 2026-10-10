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
    /// Distance between the requested size and this directory (spec: DirectorySizeDistance).
    fn distance(&self, size: u32) -> u32 {
        match self.kind {
            DirKind::Fixed => self.size.abs_diff(size),
            DirKind::Scalable if size < self.min => self.min - size,
            DirKind::Scalable if size > self.max => size - self.max,
            DirKind::Scalable => 0,
            DirKind::Threshold if size + self.threshold < self.size => self.min.saturating_sub(size),
            DirKind::Threshold if size > self.size + self.threshold => size.saturating_sub(self.max),
            DirKind::Threshold => 0,
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
        let bases = icon_bases();
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

        let mut pixmaps = vec![PathBuf::from("/usr/share/pixmaps")];
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

/// Converts _NET_WM_ICON data (several ARGB images) to the best fitting pixmap.
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
    let path = home_dir().join(".config/xfce4/xfconf/xfce-perchannel-xml/xsettings.xml");
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
    }
}
