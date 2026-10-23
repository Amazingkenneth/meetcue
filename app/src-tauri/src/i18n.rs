//! Tray menu and native dialog strings, read from the same locale files as
//! the interface.

use serde_json::Value;
use std::sync::OnceLock;

const EN: &str = include_str!("../../ui/locales/en.json");
const ZH_CN: &str = include_str!("../../ui/locales/zh-CN.json");

fn table(lang: &str) -> &'static Value {
    static EN_T: OnceLock<Value> = OnceLock::new();
    static ZH_T: OnceLock<Value> = OnceLock::new();
    if lang.starts_with("zh") {
        ZH_T.get_or_init(|| serde_json::from_str(ZH_CN).expect("zh-CN.json"))
    } else {
        EN_T.get_or_init(|| serde_json::from_str(EN).expect("en.json"))
    }
}

fn lookup<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    key.split('.').try_fold(v, |v, k| v.get(k))?.as_str()
}

/// Translates `key` (dotted path), falling back to English, then to the key.
pub fn t(lang: &str, key: &str) -> String {
    lookup(table(lang), key)
        .or_else(|| lookup(table("en"), key))
        .unwrap_or(key)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tray_strings_exist() {
        for key in [
            "tray.start",
            "tray.pause",
            "tray.resume",
            "tray.settings",
            "tray.practice",
            "tray.panel",
            "tray.resetPanel",
            "tray.exportDiagnostics",
            "tray.help",
            "tray.quit",
        ] {
            assert_ne!(t("en", key), key);
            assert_ne!(t("zh-CN", key), key);
        }
        assert_eq!(t("zh-CN", "tray.quit"), "退出 MeetCue");
        assert_eq!(t("en", "no.such.key"), "no.such.key");
    }
}
