use crate::{HttpClient, ProviderError};
use base64::Engine;
use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub const MAX_IMAGE_BYTES: usize = 12 * 1024 * 1024;
pub const MAX_DECODED_MEGAPIXEL: u32 = 40;

pub struct MediaCache {
    dir: PathBuf,
    max_bytes: u64,
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
    } else if bytes.starts_with(b"<svg") || bytes.starts_with(b"<?xml") {
        Some("image/svg+xml")
    } else {
        None
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
        Ok(Self { dir, max_bytes })
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
        let (_, bytes) = client.fetch_raw(url).await.ok()?;
        if bytes.len() > MAX_IMAGE_BYTES || is_tracking_pixel(&bytes) {
            return None;
        }
        let mime = magic_mime(&bytes)?;
        let p = self.path_for(url);
        let mut f = std::fs::File::create(&p).ok()?;
        f.write_all(&bytes).ok()?;
        drop(f);
        Some((bytes, mime))
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
        if total <= self.max_bytes {
            return;
        }
        entries.sort_by_key(|(_, _, t)| *t);
        let mut used = total;
        for (path, len, _) in entries {
            if used <= self.max_bytes {
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
    let _ = p;
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
