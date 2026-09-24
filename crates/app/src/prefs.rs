#[derive(Clone, Debug)]
pub struct Prefs {
    pub auto_read: bool,
    pub compact: bool,
    pub thumbs: bool,
    pub reader_font: f64,
    pub reader_measure: u32,
    pub reader_line_height: f64,
    pub theme: String,
    pub letter_shortcuts: bool,
    pub refresh_min: i64,
    pub retention_days: i64,
    pub media_mb: i64,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            auto_read: true,
            compact: false,
            thumbs: true,
            reader_font: 18.0,
            reader_measure: 68,
            reader_line_height: 1.6,
            theme: "system".to_string(),
            letter_shortcuts: true,
            refresh_min: 30,
            retention_days: 90,
            media_mb: 512,
        }
    }
}

impl Prefs {
    pub fn load(get: &dyn Fn(&str) -> Option<String>) -> Self {
        let mut p = Self::default();
        if let Some(v) = get("auto_read") {
            p.auto_read = v == "1";
        }
        if let Some(v) = get("compact") {
            p.compact = v == "1";
        }
        if let Some(v) = get("thumbs") {
            p.thumbs = v == "1";
        }
        if let Some(v) = get("reader_font") {
            if let Ok(f) = v.parse() {
                p.reader_font = f;
            }
        }
        if let Some(v) = get("reader_measure") {
            if let Ok(f) = v.parse() {
                p.reader_measure = f;
            }
        }
        if let Some(v) = get("reader_line_height") {
            if let Ok(f) = v.parse() {
                p.reader_line_height = f;
            }
        }
        if let Some(v) = get("theme") {
            p.theme = v;
        }
        if let Some(v) = get("letter_shortcuts") {
            p.letter_shortcuts = v == "1";
        }
        if let Some(v) = get("refresh_min") {
            if let Ok(f) = v.parse() {
                p.refresh_min = f;
            }
        }
        if let Some(v) = get("retention_days") {
            if let Ok(f) = v.parse() {
                p.retention_days = f;
            }
        }
        if let Some(v) = get("media_mb") {
            if let Ok(f) = v.parse() {
                p.media_mb = f;
            }
        }
        p
    }

    pub fn store(&self, set: &dyn Fn(&str, &str)) {
        set("auto_read", if self.auto_read { "1" } else { "0" });
        set("compact", if self.compact { "1" } else { "0" });
        set("thumbs", if self.thumbs { "1" } else { "0" });
        set("reader_font", &self.reader_font.to_string());
        set("reader_measure", &self.reader_measure.to_string());
        set("reader_line_height", &self.reader_line_height.to_string());
        set("theme", &self.theme);
        set("letter_shortcuts", if self.letter_shortcuts { "1" } else { "0" });
        set("refresh_min", &self.refresh_min.to_string());
        set("retention_days", &self.retention_days.to_string());
        set("media_mb", &self.media_mb.to_string());
    }
}
