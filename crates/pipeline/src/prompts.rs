//! Prompt templates (`prompts/*.txt`) and rendering.
//!
//! Templates are written in English for the most reliable instruction
//! following; `{output_language}` selects the language of the output.

pub const DETECT: &str = include_str!("../prompts/detect.txt");
pub const HINTS_SYSTEM: &str = include_str!("../prompts/hints.txt");
pub const HINTS_USER: &str = include_str!("../prompts/hints_user.txt");
pub const FOLLOW_UP: &str = include_str!("../prompts/followup.txt");
pub const CLASSIFY: &str = include_str!("../prompts/classify.txt");
pub const REVIEW: &str = include_str!("../prompts/review.txt");

/// Token budgets per prompt.
pub const DETECT_MAX_TOKENS: u32 = 200;
pub const HINTS_MAX_TOKENS: u32 = 700;
pub const FOLLOW_UP_MAX_TOKENS: u32 = 120;
pub const REVIEW_MAX_TOKENS: u32 = 300;

/// Replaces `{name}` placeholders. Unknown placeholders are left untouched so
/// literal braces in the JSON examples survive.
pub fn render(template: &str, vars: &[(&str, &str)]) -> String {
    let mut out = template.to_string();
    for (name, value) in vars {
        out = out.replace(&format!("{{{name}}}"), value);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_known_placeholders_only() {
        let s = render(
            HINTS_SYSTEM,
            &[
                ("role", "SRE"),
                ("materials", ""),
                ("output_language", "English"),
            ],
        );
        assert!(s.contains("The user's role: SRE"));
        assert!(s.contains("Write everything in English."));
        assert!(s.contains(r#"{"n":1,"head":"key phrase","rest":"supporting words"}"#));
        assert!(!s.contains("{role}"));
    }

    #[test]
    fn templates_have_their_placeholders() {
        assert!(DETECT.contains("{output_language}"));
        assert!(HINTS_USER.contains("{transcript}") && HINTS_USER.contains("{question}"));
        assert!(HINTS_USER.contains("{knowledge}") && HINTS_SYSTEM.contains("{materials}"));
        assert!(FOLLOW_UP.contains("{answer}"));
        assert!(REVIEW.contains("{reference}"));
    }

    /// Writes the built-in hint prompts for the UI's browser preview.
    #[test]
    fn export_defaults_for_preview() {
        if let Ok(dir) = std::env::var("TS_RS_EXPORT_DIR") {
            let path = std::path::Path::new(&dir).join("../mock/default-prompts.json");
            if path.parent().is_some_and(|p| p.is_dir()) {
                let json =
                    serde_json::json!({ "hintsSystem": HINTS_SYSTEM, "hintsUser": HINTS_USER });
                std::fs::write(path, serde_json::to_string_pretty(&json).unwrap() + "\n").unwrap();
            }
        }
    }
}
