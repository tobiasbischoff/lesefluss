use crate::{HttpClient, ProviderError};
use base64::Engine;
use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub const MAX_IMAGE_BYTES: usize = 12 * 1024 * 1024;
pub const MAX_DECODED_MEGAPIXEL: u64 = 40;
pub const MAX_PARALLEL_IMAGE_FETCHES: usize = 4;

pub struct MediaCache {
    dir: PathBuf,
    max_bytes: std::sync::atomic::AtomicU64,
    semaphore: tokio::sync::Semaphore,
}

fn magic_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if bytes.starts_with(&[0x89, 0x50, 0x4e, 0x47]) {
        Some("image/png")
    } else if bytes.starts_with(b"GIF8") {
        Some("image/gif")
    } else if bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

/// Bildmaße aus dem Dateikopf, ohne Dekodierung der Pixel.
/// Rückgabe `None` bei unbekanntem Format; SVG wird bewusst nicht zugelassen.
pub fn dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.starts_with(&[0x89, 0x50, 0x4e, 0x47]) && bytes.len() > 24 {
        let w = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
        let h = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
        return Some((w, h));
    }
    if bytes.starts_with(b"GIF8") && bytes.len() > 10 {
        let w = u16::from_le_bytes([bytes[6], bytes[7]]) as u32;
        let h = u16::from_le_bytes([bytes[8], bytes[9]]) as u32;
        return Some((w, h));
    }
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return jpeg_dimensions(bytes);
    }
    if bytes.len() > 30 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return webp_dimensions(bytes);
    }
    None
}

fn jpeg_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    let mut i = 2usize;
    while i + 9 < bytes.len() {
        if bytes[i] != 0xff {
            i += 1;
            continue;
        }
        let marker = bytes[i + 1];
        let len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
        let is_sof = matches!(marker, 0xc0..=0xcf) && marker != 0xc4 && marker != 0xc8 && marker != 0xcc;
        if is_sof {
            let h = u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]) as u32;
            let w = u16::from_be_bytes([bytes[i + 7], bytes[i + 8]]) as u32;
            return Some((w, h));
        }
        i += 2 + len.max(2);
    }
    None
}

fn webp_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    match &bytes[12..16] {
        b"VP8 " => Some((
            (u16::from_le_bytes([bytes[26], bytes[27]]) & 0x3fff) as u32,
            (u16::from_le_bytes([bytes[28], bytes[29]]) & 0x3fff) as u32,
        )),
        b"VP8L" => {
            let bits = u32::from_le_bytes([bytes[21], bytes[22], bytes[23], bytes[24]]);
            Some(((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1))
        }
        b"VP8X" => Some((
            (bytes[24] as u32 | (bytes[25] as u32) << 8 | (bytes[26] as u32) << 16) + 1,
            (bytes[27] as u32 | (bytes[28] as u32) << 8 | (bytes[29] as u32) << 16) + 1,
        )),
        _ => None,
    }
}

pub fn image_acceptable(bytes: &[u8]) -> bool {
    if bytes.len() > MAX_IMAGE_BYTES || is_tracking_pixel(bytes) {
        return false;
    }
    if magic_mime(bytes).is_none() {
        return false;
    }
    match dimensions(bytes) {
        Some((w, h)) => (w as u64) * (h as u64) <= MAX_DECODED_MEGAPIXEL * 1_000_000,
        None => false,
    }
}

fn is_tracking_pixel(bytes: &[u8]) -> bool {
    bytes.len() <= 128
}

pub fn key_of(url: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    url.hash(&mut h);
    format!("{:016x}", h.finish())
}

impl MediaCache {
    pub fn new(dir: PathBuf, max_bytes: u64) -> std::io::Result<Self> {
        std::fs::create_dir_all(&dir)?;
        Ok(Self {
            dir,
            max_bytes: std::sync::atomic::AtomicU64::new(max_bytes),
            semaphore: tokio::sync::Semaphore::new(MAX_PARALLEL_IMAGE_FETCHES),
        })
    }

    pub fn path_for(&self, url: &str) -> PathBuf {
        self.dir.join(key_of(url))
    }

    pub fn get_cached(&self, url: &str) -> Option<(Vec<u8>, &'static str)> {
        let p = self.path_for(url);
        let bytes = std::fs::read(&p).ok()?;
        let mime = magic_mime(&bytes)?;
        let _ = filetime_touch(&p);
        Some((bytes, mime))
    }

    pub async fn get_or_fetch(&self, client: &HttpClient, url: &str) -> Option<(Vec<u8>, &'static str)> {
        if let Some(hit) = self.get_cached(url) {
            return Some(hit);
        }
        let _slot = self.semaphore.acquire().await.ok()?;
        if let Some(hit) = self.get_cached(url) {
            return Some(hit);
        }
        let (_, bytes) = client.fetch_image(url).await.ok()?;
        if !image_acceptable(&bytes) {
            return None;
        }
        let mime = magic_mime(&bytes)?;
        let p = self.path_for(url);
        let tmp = p.with_extension("part");
        let mut f = std::fs::File::create(&tmp).ok()?;
        f.write_all(&bytes).ok()?;
        f.sync_all().ok()?;
        drop(f);
        std::fs::rename(&tmp, &p).ok()?;
        self.prune(&HashSet::new());
        Some((bytes, mime))
    }

    pub fn set_max_bytes(&self, b: u64) {
        self.max_bytes.store(b, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn total_bytes(&self) -> u64 {
        std::fs::read_dir(&self.dir)
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .filter_map(|e| e.metadata().ok())
                    .map(|m| m.len())
                    .sum()
            })
            .unwrap_or(0)
    }

    pub fn prune(&self, pinned: &HashSet<String>) {
        let mut entries: Vec<(PathBuf, u64, SystemTime)> = Vec::new();
        let Ok(rd) = std::fs::read_dir(&self.dir) else { return };
        for e in rd.filter_map(|e| e.ok()) {
            let Ok(m) = e.metadata() else { continue };
            let modified = m.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            entries.push((e.path(), m.len(), modified));
        }
        let total: u64 = entries.iter().map(|(_, l, _)| l).sum();
        let cap = self.max_bytes.load(std::sync::atomic::Ordering::Relaxed);
        if total <= cap {
            return;
        }
        entries.sort_by_key(|(_, _, t)| *t);
        let mut used = total;
        for (path, len, _) in entries {
            if used <= cap {
                break;
            }
            let name = path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            if pinned.contains(&name) {
                continue;
            }
            if std::fs::remove_file(&path).is_ok() {
                used -= len;
            }
        }
    }
}

fn filetime_touch(p: &Path) -> std::io::Result<()> {
    let file = std::fs::OpenOptions::new().append(true).open(p)?;
    file.set_len(file.metadata()?.len())?;
    Ok(())
}

pub fn placeholder_data_uri(alt: &str) -> String {
    let alt: String = alt.chars().take(60).collect();
    let esc = alt.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let svg = format!(
        "<svg xmlns='http://www.w3.org/2000/svg' width='640' height='96'><rect width='100%' height='100%' fill='#26272b'/><text x='50%' y='50%' fill='#9a9da6' font-family='sans-serif' font-size='13' text-anchor='middle' dominant-baseline='middle'>Bild nicht verfügbar — {esc}</text></svg>"
    );
    let enc = url::form_urlencoded::byte_serialize(svg.as_bytes()).collect::<String>();
    format!("data:image/svg+xml;utf8,{enc}")
}

pub fn data_uri(bytes: &[u8], mime: &str) -> String {
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    format!("data:{mime};base64,{b64}")
}

pub fn cache_dir() -> PathBuf {
    let base = std::env::var("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            PathBuf::from(home).join(".cache")
        });
    base.join("lesefluss").join("media")
}

#[allow(dead_code)]
fn err_unused(e: ProviderError) -> String {
    e.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut v = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        v.extend_from_slice(&13u32.to_be_bytes());
        v.extend_from_slice(b"IHDR");
        v.extend_from_slice(&w.to_be_bytes());
        v.extend_from_slice(&h.to_be_bytes());
        v.extend_from_slice(&[0x08, 0x06, 0, 0, 0]);
        v.extend_from_slice(&[0u8; 512]);
        v
    }

    #[test]
    fn svg_and_xml_are_never_treated_as_images() {
        let svg = b"<svg xmlns='http://www.w3.org/2000/svg'></svg>";
        assert!(magic_mime(svg).is_none());
        assert!(!image_acceptable(svg));
        assert!(!image_acceptable(b"<?xml version='1.0'?><svg/>"));
    }

    #[test]
    fn oversized_images_are_rejected_by_header_dimensions() {
        let bomb = png(20_000, 20_000);
        assert_eq!(dimensions(&bomb), Some((20_000, 20_000)));
        assert!(!image_acceptable(&bomb), "400 MP werden abgelehnt");
        assert!(image_acceptable(&png(1200, 800)));
    }

    #[test]
    fn tracking_pixels_and_unknown_formats_are_rejected() {
        assert!(!image_acceptable(&[0u8; 32]));
        assert!(!image_acceptable(b"not an image at all"));
    }
}
