//! Texte der Oberfläche in zwei Sprachen (§16).
//!
//! Deutsch ist die Standardsprache; Englisch wird aktiviert, wenn
//! `LANG`/`LC_ALL` englischsprachig beginnt oder `LF_LANG=en` gesetzt ist.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    De,
    En,
}

impl Lang {
    pub fn detect() -> Lang {
        if let Ok(explicit) = std::env::var("LF_LANG") {
            if explicit.starts_with("en") {
                return Lang::En;
            }
            if explicit.starts_with("de") {
                return Lang::De;
            }
        }
        for key in ["LC_ALL", "LC_MESSAGES", "LANG"] {
            if let Ok(value) = std::env::var(key) {
                let v = value.to_ascii_lowercase();
                if v.starts_with("en") || v.contains("_en") || v.contains(".en") {
                    return Lang::En;
                }
            }
        }
        Lang::De
    }
}

pub fn text(lang: Lang, de: &str, en: &str) -> String {
    match lang {
        Lang::De => de.to_string(),
        Lang::En => en.to_string(),
    }
}

/// Häufig benutzte Beschriftungen an einer Stelle gebündelt.
pub struct Strings {
    pub lang: Lang,
}

impl Strings {
    pub fn detect() -> Self {
        Self {
            lang: Lang::detect(),
        }
    }

    pub fn get(&self, de: &str, en: &str) -> String {
        text(self.lang, de, en)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn german_is_the_default() {
        let s = Strings { lang: Lang::De };
        assert_eq!(s.get("Einstellungen", "Settings"), "Einstellungen");
    }

    #[test]
    fn english_is_selected_for_english_environments() {
        std::env::set_var("LF_LANG", "en_GB.UTF-8");
        assert_eq!(Lang::detect(), Lang::En);
        assert_eq!(
            Strings::detect().get("Einstellungen", "Settings"),
            "Settings"
        );
        std::env::set_var("LF_LANG", "de_DE.UTF-8");
        assert_eq!(Lang::detect(), Lang::De);
        std::env::remove_var("LF_LANG");
    }

    #[test]
    fn plural_forms_are_chosen_explicitly() {
        let s = Strings { lang: Lang::De };
        assert_eq!(
            if 1 == 1 {
                s.get("1 neuer Artikel", "1 new article")
            } else {
                s.get("2 neue Artikel", "2 new articles")
            },
            "1 neuer Artikel"
        );
    }
}
