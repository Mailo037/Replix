//! Replix in other languages. English is the source: a text is looked up by its English
//! wording in `src/locales/<code>.json`, the same files the page uses, and stays English
//! where a translation is missing. `{name}` in a text is filled in by `trf`.

use std::collections::HashMap;
use std::sync::OnceLock;

use parking_lot::RwLock;

/// Every language Replix speaks, by code, with its catalog. English has entries only
/// where a text has plural forms.
const CATALOGS: &[(&str, &str)] = &[
    ("en", include_str!("../../src/locales/en.json")),
    ("de", include_str!("../../src/locales/de.json")),
    ("es", include_str!("../../src/locales/es.json")),
    ("fr", include_str!("../../src/locales/fr.json")),
    ("it", include_str!("../../src/locales/it.json")),
    ("pt-BR", include_str!("../../src/locales/pt-BR.json")),
    ("pl", include_str!("../../src/locales/pl.json")),
    ("ru", include_str!("../../src/locales/ru.json")),
    ("tr", include_str!("../../src/locales/tr.json")),
    ("ja", include_str!("../../src/locales/ja.json")),
    ("ko", include_str!("../../src/locales/ko.json")),
    ("zh-CN", include_str!("../../src/locales/zh-CN.json")),
];

type Catalog = HashMap<String, String>;

/// A text in a catalog is a string, or its plural forms by category ("one", "few",
/// "other" and so on). Replix's own texts need no plurals, so they take "other".
fn parse(json: &str) -> Catalog {
    let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(json) else {
        return Catalog::new();
    };
    map.into_iter()
        .filter_map(|(key, value)| match value {
            serde_json::Value::String(s) => Some((key, s)),
            serde_json::Value::Object(forms) => forms.get("other").and_then(|v| v.as_str()).map(|s| (key, s.to_string())),
            _ => None,
        })
        .collect()
}

fn catalogs() -> &'static HashMap<&'static str, Catalog> {
    static PARSED: OnceLock<HashMap<&'static str, Catalog>> = OnceLock::new();
    PARSED.get_or_init(|| CATALOGS.iter().map(|(code, json)| (*code, parse(json))).collect())
}

static CURRENT: RwLock<&'static str> = parking_lot::const_rwlock("en");

/// The language a setting stands for: "auto" follows the system.
pub fn resolve(setting: &str) -> &'static str {
    if setting == "auto" || setting.is_empty() {
        matching(&crate::platform::ui_language())
    } else {
        matching(setting)
    }
}

/// The closest language Replix has to a locale name like "de-AT" or "pt-PT".
fn matching(locale: &str) -> &'static str {
    let lower = locale.to_lowercase().replace('_', "-");
    if let Some((code, _)) = CATALOGS.iter().find(|(code, _)| code.to_lowercase() == lower) {
        return code;
    }
    let language = lower.split('-').next().unwrap_or("");
    CATALOGS
        .iter()
        .map(|(code, _)| *code)
        .find(|code| code.split('-').next() == Some(language))
        .unwrap_or("en")
}

/// Switches the language; says whether it changed.
pub fn set(setting: &str) -> bool {
    let next = resolve(setting);
    let mut current = CURRENT.write();
    let changed = *current != next;
    *current = next;
    changed
}

pub fn current() -> &'static str {
    *CURRENT.read()
}

/// A text in the current language.
pub fn tr(text: &str) -> String {
    catalogs().get(current()).and_then(|c| c.get(text)).filter(|t| !t.is_empty()).cloned().unwrap_or_else(|| text.to_string())
}

/// A text in the current language with its `{name}` parts filled in.
pub fn trf(text: &str, args: &[(&str, &dyn std::fmt::Display)]) -> String {
    let mut out = tr(text);
    for (name, value) in args {
        out = out.replace(&format!("{{{name}}}"), &value.to_string());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placeholders(text: &str) -> std::collections::BTreeSet<String> {
        let mut out = std::collections::BTreeSet::new();
        let mut rest = text;
        while let Some(open) = rest.find('{') {
            let Some(close) = rest[open..].find('}') else { break };
            out.insert(rest[open + 1..open + close].to_string());
            rest = &rest[open + close + 1..];
        }
        out
    }

    /// Every catalog reads, and no translation loses or invents a `{name}`: a missing one
    /// would drop a game's name or a count from a message.
    #[test]
    fn catalogs_keep_their_placeholders() {
        for (code, json) in CATALOGS {
            let value: serde_json::Value = serde_json::from_str(json).unwrap_or_else(|e| panic!("{code}.json: {e}"));
            let map = value.as_object().unwrap_or_else(|| panic!("{code}.json is not an object"));
            for (key, text) in map {
                let wanted = placeholders(key);
                let forms: Vec<&str> = match text {
                    serde_json::Value::String(s) => vec![s.as_str()],
                    serde_json::Value::Object(f) => f.values().filter_map(|v| v.as_str()).collect(),
                    _ => panic!("{code}.json: {key:?} is neither text nor plural forms"),
                };
                for form in forms {
                    assert!(placeholders(form).is_subset(&wanted), "{code}.json: {key:?} -> {form:?} has other placeholders");
                }
            }
        }
    }

    #[test]
    fn locales_match_the_closest_language() {
        assert_eq!(matching("de-AT"), "de");
        assert_eq!(matching("pt-PT"), "pt-BR");
        assert_eq!(matching("zh-Hans-CN"), "zh-CN");
        assert_eq!(matching("PT_br"), "pt-BR");
        assert_eq!(matching("sv-SE"), "en");
        assert_eq!(resolve("fr"), "fr");
    }

    #[test]
    fn missing_texts_stay_english_and_fill_in() {
        assert_eq!(trf("{name} · {n}", &[("name", &"Game"), ("n", &3)]), "Game · 3");
    }
}
