use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Progress / log line emitted to GUI or CLI (`kind` matches frontend listener).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum FlashProgress {
    #[serde(rename = "percent")]
    Percent { value: u8 },
    #[serde(rename = "log_line")]
    LogLine { line: String },
    /// Structured log event: `key` is a frontend i18n key (e.g. `"flash.log.esp.connected"`),
    /// `params` holds named substitution values. The GUI translates using the user's locale;
    /// the CLI formats a plain-English fallback.
    #[serde(rename = "log_key")]
    LogKey {
        key: String,
        #[serde(default, skip_serializing_if = "HashMap::is_empty")]
        params: HashMap<String, String>,
    },
    #[serde(rename = "phase")]
    Phase { name: String },
    #[serde(rename = "done")]
    Done { ok: bool, message: Option<String> },
}

#[cfg(test)]
mod tests {
    use super::FlashProgress;

    #[test]
    fn done_failure_json_has_ok_false_and_message() {
        let p = FlashProgress::Done {
            ok: false,
            message: Some("invalid job: example".into()),
        };
        let v = serde_json::to_value(&p).expect("serialize");
        assert_eq!(v["kind"], "done");
        assert_eq!(v["ok"], false, "ok must be JSON false, not omitted");
        assert_eq!(v["message"].as_str(), Some("invalid job: example"));
    }
}
