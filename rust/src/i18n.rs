//! Interface language: Portuguese (Brazil) or English.
//!
//! Default: the Windows language (Portuguese -> pt-BR; anything else ->
//! English). The user can pin either one under Settings.
//!
//! The texts live in the code itself, side by side:
//! `tr!("Conectar", "Connect")` returns &'static str and
//! `trf!("{n} de {total}", "{n} of {total}")` builds a String (format!).

use std::sync::atomic::{AtomicBool, Ordering};

/// Fixed text in the current language.
macro_rules! tr {
    ($pt:literal, $en:literal $(,)?) => {
        if $crate::i18n::pt() {
            $pt
        } else {
            $en
        }
    };
}

/// Formatted text (like format!) in the current language.
macro_rules! trf {
    ($pt:literal, $en:literal $(, $arg:expr)* $(,)?) => {
        if $crate::i18n::pt() {
            format!($pt $(, $arg)*)
        } else {
            format!($en $(, $arg)*)
        }
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    Portuguese,
    English,
}

impl Language {
    /// Code stored in the settings.
    #[cfg(test)]
    pub fn code(self) -> &'static str {
        match self {
            Language::Portuguese => "pt",
            Language::English => "en",
        }
    }

    pub fn from_code(c: &str) -> Option<Language> {
        match c {
            "pt" => Some(Language::Portuguese),
            "en" => Some(Language::English),
            _ => None,
        }
    }
}

// When not initialized (tests), it stays in Portuguese.
static PORTUGUESE: AtomicBool = AtomicBool::new(true);

pub fn pt() -> bool {
    PORTUGUESE.load(Ordering::Relaxed)
}

/// Applies the user's choice (None = automatic from Windows).
/// IDLERX_LANGUAGE=pt|en forces a language (documentation screenshots).
pub fn apply(choice: Option<Language>) {
    let forced = std::env::var("IDLERX_LANGUAGE")
        .ok()
        .and_then(|c| Language::from_code(&c));
    let language = forced.or(choice).unwrap_or_else(from_windows);
    PORTUGUESE.store(language == Language::Portuguese, Ordering::Relaxed);
}

/// Windows interface language.
pub fn from_windows() -> Language {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetUserDefaultUILanguage() -> u16;
    }
    from_langid(unsafe { GetUserDefaultUILanguage() })
}

/// Windows LANGID -> app language: Portuguese (Brazil or Portugal)
/// uses pt-BR; all others, English.
fn from_langid(langid: u16) -> Language {
    const LANG_PORTUGUESE: u16 = 0x16;
    if langid & 0x3FF == LANG_PORTUGUESE {
        Language::Portuguese
    } else {
        Language::English
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_from_windows() {
        assert_eq!(from_langid(0x0416), Language::Portuguese); // pt-BR
        assert_eq!(from_langid(0x0816), Language::Portuguese); // pt-PT
        assert_eq!(from_langid(0x0409), Language::English); // en-US
        assert_eq!(from_langid(0x0C0A), Language::English); // es-ES
        assert_eq!(from_langid(0x0407), Language::English); // de-DE
    }

    #[test]
    fn codes() {
        for i in [Language::Portuguese, Language::English] {
            assert_eq!(Language::from_code(i.code()), Some(i));
        }
        assert_eq!(Language::from_code("xx"), None);
    }

    #[test]
    fn macros_default_to_portuguese() {
        let n = 2;
        assert_eq!(tr!("Conectar", "Connect"), "Conectar");
        assert_eq!(trf!("{n} contas", "{n} accounts"), "2 contas");
        assert_eq!(trf!("{} de {}", "{} of {}", 1, n), "1 de 2");
    }
}
