//! Which language the bridge's user-visible text is written in.
//!
//! Two languages only (Chinese and English), decided once at startup and then
//! passed down as a value — the tray shell has no settings UI to switch it from,
//! and its muda/tray-icon handles are not `Send`, so there is nothing for a
//! runtime switch to hook into. Autostart makes a restart the natural way to
//! pick up a changed system language.

/// The two languages the bridge speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Zh,
    En,
}

/// Pick a language for the system's locale tag.
///
/// The same rule the desktop GUI already applies to `navigator.language`
/// (`resolveLocale` in `src/stores/settings.ts`): anything Chinese gets Chinese,
/// everything else — including locales the bridge has no translation for, and an
/// unknown/empty tag — gets English. Deliberately not a per-language table: an
/// unsupported locale must land on the one language every reader of this tool's
/// docs can fall back to, not on Chinese by accident of being the default.
pub fn detect_lang(locale: &str) -> Lang {
    if locale.starts_with("zh") {
        Lang::Zh
    } else {
        Lang::En
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_chinese_locales_get_chinese_everything_else_gets_english() {
        // The three shapes a Chinese tag actually arrives in: BCP-47 from
        // CFLocale / GetUserDefaultLocaleName, the POSIX form with an encoding
        // suffix, and the script-only form (macOS reports `zh-Hans` when the
        // user picked Simplified Chinese without a region).
        assert_eq!(detect_lang("zh-CN"), Lang::Zh, "the plain BCP-47 tag");
        assert_eq!(
            detect_lang("zh_CN.UTF-8"),
            Lang::Zh,
            "the POSIX form with an encoding suffix must still read as Chinese"
        );
        assert_eq!(
            detect_lang("zh-Hans"),
            Lang::Zh,
            "a script subtag with no region must still read as Chinese"
        );

        assert_eq!(detect_lang("en-US"), Lang::En, "the plain English tag");
        assert_eq!(
            detect_lang(""),
            Lang::En,
            "an unknown locale must fall back to English, never to Chinese"
        );
        // Regression cases: the tool ships no translation for either, and
        // "not translated" must resolve to English rather than to the language
        // that happens to be first in the source.
        assert_eq!(
            detect_lang("ja-JP"),
            Lang::En,
            "an untranslated locale must fall back to English"
        );
        assert_eq!(
            detect_lang("ko-KR"),
            Lang::En,
            "an untranslated locale must fall back to English"
        );
    }
}
