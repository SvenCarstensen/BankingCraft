//! UI language only; never use translated labels as persisted IDs or protocol values.
extern crate self as i18n;
use std::sync::atomic::{AtomicU8, Ordering};
pub use bankingcraft_i18n_macros::tr_format;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Language { German, English, French, Spanish }
impl Language {
    pub fn from_locale(locale: &str) -> Self {
        match locale.trim().split(['-', '_', '.', '@']).next().unwrap_or("").to_ascii_lowercase().as_str() {
            "de" => Self::German, "fr" => Self::French, "es" => Self::Spanish, _ => Self::English,
        }
    }
    pub fn code(self) -> &'static str {
        match self { Self::German => "de", Self::English => "en", Self::French => "fr", Self::Spanish => "es" }
    }
}
// Source language until the application installs the persisted/system preference.
// Keeping initialization explicit also lets existing protocol tests use source messages.
static LANGUAGE: AtomicU8 = AtomicU8::new(Language::German as u8);
#[cfg(any(test, feature = "test-support"))]
thread_local! { static OVERRIDE: std::cell::Cell<Option<Language>> = const { std::cell::Cell::new(None) }; }
/// Isolate localization tests from other threads, including when an assertion panics.
#[cfg(any(test, feature = "test-support"))]
pub fn with_language<T>(language: Language, action: impl FnOnce() -> T) -> T {
    struct Restore(Option<Language>);
    impl Drop for Restore { fn drop(&mut self) { OVERRIDE.with(|v| v.set(self.0)); } }
    let _restore = Restore(OVERRIDE.with(|v| v.replace(Some(language))));
    action()
}
pub fn language() -> Language {
    #[cfg(any(test, feature = "test-support"))]
    if let Some(language) = OVERRIDE.with(|v| v.get()) { return language; }
    match LANGUAGE.load(Ordering::Relaxed) { 0 => Language::German, 2 => Language::French, 3 => Language::Spanish, _ => Language::English }
}
pub fn set_language(language: Language) { LANGUAGE.store(language as u8, Ordering::Relaxed); }
include!(concat!(env!("OUT_DIR"), "/catalogue.rs"));
pub fn tr(key: &str) -> &str { text(language(), key) }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn system_language_supports_regions_and_defaults_to_english() {
        for (locale, expected) in [("de-DE", Language::German), ("de_AT.UTF-8", Language::German),
            ("EN-gb", Language::English), ("fr-CA", Language::French), ("es_MX", Language::Spanish),
            ("pt-BR", Language::English), ("C", Language::English), ("", Language::English), ("zh-Hans", Language::English)] {
            assert_eq!(Language::from_locale(locale), expected, "{locale}");
        }
    }
    #[test]
    fn every_catalogue_entry_has_all_languages_and_preserves_placeholders() {
        let entries: std::collections::BTreeMap<String, [String; 3]> = serde_json::from_str(include_str!("../catalogue.json")).unwrap();
        fn slots(s: &str) -> Vec<&str> {
            let mut result = Vec::new(); let mut tail = s;
            while let Some((_, after)) = tail.split_once('{') {
                let Some((slot, rest)) = after.split_once('}') else { panic!("unclosed placeholder") };
                result.push(slot); tail = rest;
            }
            result.sort(); result
        }
        for (key, values) in &entries {
            for (lang, value) in [Language::English, Language::French, Language::Spanish].into_iter().zip(values) {
                assert!(!value.is_empty(), "{key}");
                assert_eq!(text(lang, key), value, "{key}");
                assert_eq!(slots(key), slots(value), "placeholders: {key} / {}", lang.code());
            }
            assert_eq!(text(Language::German, key), key);
        }
        assert_eq!(text(Language::French, "User-created group"), "User-created group");
    }
}

#[cfg(test)]
mod format_tests {
    use super::*;
    #[test]
    fn translated_formats_preserve_values_unicode_and_thread_isolation() {
        for (lang, expected) in [(Language::German, "Überweisung 12,34 € an Zoë"), (Language::English, "Transfer 12,34 € to Zoë"), (Language::French, "Virement de 12,34 € vers Zoë"), (Language::Spanish, "Transferencia de 12,34 € a Zoë")] {
            with_language(lang, || {
                assert_eq!(tr_format!("Überweisung {} an {}", "12,34 €", "Zoë"), expected);
                let code = "HKCDB";
                assert!(tr_format!("Keine gemeinsame Version für {code}").contains(code));
                let sw = 0x63c2;
                let rendered = tr_format!("Die Chipkarte hat die PIN abgelehnt. Verbleibende PIN-Versuche: {} ({sw:04X}). Keine automatische Wiederholung.", 2);
                assert!(rendered.contains("2 (63C2)"));
                with_language(Language::French, || assert_eq!(tr("Einstellungen"), "Paramètres"));
                assert_eq!(language(), lang);
            });
        }
        assert_eq!(language(), Language::German);
        with_language(Language::French, || {
            assert_eq!(std::thread::spawn(language).join().unwrap(), Language::German);
        });
    }
}
