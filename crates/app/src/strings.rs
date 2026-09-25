//! Application language, resolved once at startup. Changes apply on next launch.
use std::sync::OnceLock;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    De,
    En,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum LanguageChoice {
    #[default]
    English,
    German,
    System,
}

impl LanguageChoice {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "en" => Some(Self::English),
            "de" => Some(Self::German),
            "system" => Some(Self::System),
            _ => None,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::German => "de",
            Self::System => "system",
        }
    }

    pub fn resolve(self, get: &impl Fn(&str) -> Option<String>) -> Lang {
        match self {
            Self::English => Lang::En,
            Self::German => Lang::De,
            Self::System => {
                // POSIX precedence: an explicit unsupported locale must not fall
                // through to a lower-priority German locale. C/POSIX use English.
                let locale = ["LC_ALL", "LC_MESSAGES", "LANG"]
                    .iter()
                    .filter_map(|key| get(key))
                    .find(|value| !value.trim().is_empty())
                    .unwrap_or_default();
                match locale
                    .split(['_', '-', '.', '@'])
                    .next()
                    .unwrap_or("")
                    .to_ascii_lowercase()
                    .as_str()
                {
                    "de" => Lang::De,
                    _ => Lang::En,
                }
            }
        }
    }
}

static LANGUAGE: OnceLock<Lang> = OnceLock::new();

pub fn current() -> Lang {
    *LANGUAGE.get().unwrap_or(&Lang::En)
}

pub fn startup_choice(
    saved: Option<&str>,
    get: &impl Fn(&str) -> Option<String>,
) -> LanguageChoice {
    // Preserve the explicit command-line override used in existing installations.
    get("LF_LANG")
        .as_deref()
        .and_then(|value| {
            LanguageChoice::parse(value.split(['_', '-', '.', '@']).next().unwrap_or(""))
        })
        .or_else(|| saved.and_then(LanguageChoice::parse))
        .unwrap_or_default()
}

pub fn initialize(saved: Option<&str>) {
    let get = |key: &str| std::env::var(key).ok();
    let _ = LANGUAGE.set(startup_choice(saved, &get).resolve(&get));
}

/// Literal pairs keep translation coverage and format checking at the call site.
#[macro_export]
macro_rules! tr {
    ($de:literal, $en:literal) => {
        match $crate::strings::current() {
            $crate::strings::Lang::De => $de,
            $crate::strings::Lang::En => $en,
        }
    };
}

#[macro_export]
macro_rules! tr_format {
    ($de:literal, $en:literal $(, $args:expr)* $(,)?) => {
        match $crate::strings::current() {
            $crate::strings::Lang::De => format!($de $(, $args)*),
            $crate::strings::Lang::En => format!($en $(, $args)*),
        }
    };
}

#[macro_export]
macro_rules! tr_plural {
    ($count:expr, $de_one:literal, $de_many:literal, $en_one:literal, $en_many:literal $(, $args:expr)* $(,)?) => {
        if $count == 1 {
            $crate::tr_format!($de_one, $en_one $(, $args)*)
        } else {
            $crate::tr_format!($de_many, $en_many $(, $args)*)
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(values: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |key| {
            values
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| v.to_string())
        }
    }

    #[test]
    fn translated_strings_in_fresh_processes() {
        const CASE: &str = "LESEFLUSS_TEST_LANGUAGE";
        if let Ok(choice) = std::env::var(CASE) {
            initialize(Some(&choice));
            let german = choice == "de" || choice == "system";
            assert_eq!(
                crate::tr!("Einstellungen", "Settings"),
                if german { "Einstellungen" } else { "Settings" }
            );
            let count = 1;
            assert_eq!(
                crate::tr_plural!(
                    count,
                    "{count} Artikel",
                    "{count} Artikel",
                    "{count} article",
                    "{count} articles"
                ),
                if german { "1 Artikel" } else { "1 article" }
            );
            let count = 2;
            assert_eq!(
                crate::tr_plural!(
                    count,
                    "{count} Artikel",
                    "{count} Artikel",
                    "{count} article",
                    "{count} articles"
                ),
                if german { "2 Artikel" } else { "2 articles" }
            );
            return;
        }
        for choice in ["en", "de", "system"] {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "strings::tests::translated_strings_in_fresh_processes",
                    "--nocapture",
                ])
                .env(CASE, choice)
                .env("LC_ALL", "de_DE.UTF-8")
                .env_remove("LF_LANG")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{choice}: {} {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    #[test]
    fn english_default_is_independent_of_system_language() {
        let get = env(&[("LANG", "de_DE.UTF-8")]);
        assert_eq!(startup_choice(None, &get).resolve(&get), Lang::En);
        assert_eq!(
            startup_choice(Some("invalid"), &get),
            LanguageChoice::English
        );
    }

    #[test]
    fn explicit_choice_and_command_line_override() {
        let get = env(&[("LANG", "en_US.UTF-8")]);
        assert_eq!(startup_choice(Some("de"), &get).resolve(&get), Lang::De);
        let get = env(&[("LF_LANG", "en_GB.UTF-8"), ("LANG", "de_DE.UTF-8")]);
        assert_eq!(startup_choice(Some("de"), &get).resolve(&get), Lang::En);
        assert_eq!(
            startup_choice(Some("system"), &env(&[])),
            LanguageChoice::System
        );
    }

    #[test]
    fn system_locale_precedence_and_unsupported_fallback() {
        for (values, expected) in [
            (vec![("LANG", "de_AT.UTF-8")], Lang::De),
            (vec![("LANG", "DE-ch")], Lang::De),
            (
                vec![("LC_ALL", ""), ("LC_MESSAGES", "de_DE"), ("LANG", "en_US")],
                Lang::De,
            ),
            (vec![("LC_ALL", "C"), ("LANG", "de_DE")], Lang::En),
            (vec![("LC_MESSAGES", "fr_FR"), ("LANG", "de_DE")], Lang::En),
            (vec![("LANG", "fr_FR.UTF-8")], Lang::En),
            (vec![], Lang::En),
        ] {
            assert_eq!(
                LanguageChoice::System.resolve(&env(&values)),
                expected,
                "{values:?}"
            );
        }
    }
}
