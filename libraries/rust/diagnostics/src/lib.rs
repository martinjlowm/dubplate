//! The finding type every analysis stage reports its doubts in.
//!
//! One type across the stages, so the CLI, the JSON and the HTML page render
//! findings the same way wherever they came from, and so a caller can act on a
//! `code` without knowing which stage raised it.

use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Worth knowing, changes nothing about whether to trust the answer.
    Info,
    /// The answer above is unsafe to use without reading the evidence.
    Warning,
}

#[derive(Clone, Debug, Serialize)]
pub struct Diagnostic {
    /// Stable identifier, so a script can act on a class of failure. Never
    /// reworded once published; the message carries the wording.
    pub code: &'static str,
    pub severity: Severity,
    pub message: String,
}

impl Diagnostic {
    pub fn warning(code: &'static str, message: impl Into<String>) -> Self {
        Diagnostic {
            code,
            severity: Severity::Warning,
            message: message.into(),
        }
    }

    pub fn info(code: &'static str, message: impl Into<String>) -> Self {
        Diagnostic {
            code,
            severity: Severity::Info,
            message: message.into(),
        }
    }
}
