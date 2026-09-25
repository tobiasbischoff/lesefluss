use reader::tokens::{Color, Tokens};
use std::collections::HashMap;
use std::path::PathBuf;

fn state_theme_path() -> PathBuf {
    let base = std::env::var("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            PathBuf::from(home).join(".local/state")
        });
    base.join("omarchy").join("current").join("theme")
}

fn config_theme_path() -> PathBuf {
    let base = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            PathBuf::from(home).join(".config")
        });
    base.join("omarchy").join("current").join("theme")
}

pub fn omarchy_colors_path() -> Option<PathBuf> {
    for p in [state_theme_path(), config_theme_path()] {
        if p.is_dir() {
            let c = p.join("colors.toml");
            if c.is_file() {
                return Some(c);
            }
        } else if p.is_file() {
            if let Ok(name) = std::fs::read_to_string(&p) {
                let name = name.trim().to_string();
                for root in [
                    "/usr/share/omarchy/themes",
                    "/usr/local/share/omarchy/themes",
                ] {
                    let c = PathBuf::from(root).join(&name).join("colors.toml");
                    if c.is_file() {
                        return Some(c);
                    }
                }
            }
        }
    }
    None
}

fn parse_hex(v: &str) -> Option<Color> {
    let v = v.trim().trim_start_matches('#');
    if v.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&v[0..2], 16).ok()?;
    let g = u8::from_str_radix(&v[2..4], 16).ok()?;
    let b = u8::from_str_radix(&v[4..6], 16).ok()?;
    Some(Color { r, g, b })
}

pub fn parse_colors_toml(text: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('[') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let v = v.trim().trim_matches('"').to_string();
        map.insert(k.trim().to_string(), v);
    }
    map
}

pub fn omarchy_tokens() -> Option<Tokens> {
    let path = omarchy_colors_path()?;
    let text = std::fs::read_to_string(&path).ok()?;
    let map = parse_colors_toml(&text);
    let dark = map.get("mode").map(|m| m != "light").unwrap_or(true);
    let get = |k: &str| map.get(k).and_then(|v| parse_hex(v));
    let background = get("background")?;
    let foreground = get("foreground")?;
    let accent = get("accent").unwrap_or(background);
    let selection = get("selection").unwrap_or(background);
    let muted = get("muted")
        .or_else(|| get("dark_foreground"))
        .unwrap_or(foreground);
    let raised = get("lighter_background").unwrap_or(background);
    let darker = get("dark_background").unwrap_or(background);
    Some(Tokens {
        dark,
        surface_reader: darker,
        surface_list: background,
        surface_raised: raised,
        text_primary: foreground,
        text_secondary: muted,
        separator: raised,
        accent,
        selection,
        focus_ring: accent,
        sidebar_top: selection,
        sidebar_bottom: background,
    })
}

pub fn watch_dir() -> Option<PathBuf> {
    let p = state_theme_path();
    if p.is_dir() {
        return Some(p.parent()?.to_path_buf());
    }
    let p = config_theme_path();
    if p.is_file() || p.is_dir() {
        return Some(p.parent()?.to_path_buf());
    }
    None
}
