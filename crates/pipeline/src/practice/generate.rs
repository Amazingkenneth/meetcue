//! Writes a practice interview from the user's role and knowledge base: the
//! questions an interviewer for that role is likely to ask, with reference
//! answers drawn from the user's own material.

use meetcue_core::settings::Material;
use meetcue_core::UsagePurpose;

use super::{PracticeItem, PracticeKind, PracticeScript};
use crate::materials;
use crate::prompts;
use crate::provider::LlmRequest;

/// How much of the knowledge base the request carries.
pub const MATERIAL_CHARS: usize = 8_000;
pub const DEFAULT_COUNT: u32 = 10;
const MAX_COUNT: u32 = 20;

/// The request for `count` questions. `lang` names the output language.
pub fn request(
    model: &str,
    role: &str,
    material: &[Material],
    count: u32,
    lang: &str,
) -> LlmRequest {
    let count = count.clamp(3, MAX_COUNT);
    let role = if role.trim().is_empty() {
        "not specified"
    } else {
        role.trim()
    };
    let text = materials::summary(material, MATERIAL_CHARS);
    let text = if text.trim().is_empty() {
        "(none)".to_string()
    } else {
        text
    };
    let system = prompts::render(
        prompts::GENERATE,
        &[
            ("count", &count.to_string()),
            ("output_language", lang),
            ("role", role),
            ("materials", &text),
        ],
    );
    LlmRequest::new(model, UsagePurpose::Generate)
        .system(system, false)
        .user("Write the questions now.")
        .max_tokens(300 + 220 * count)
        .temperature(0.7)
}

/// Reads the reply into a question list; lines that aren't questions are
/// skipped. `None` when no question could be read.
pub fn parse(reply: &str, title: &str) -> Option<PracticeScript> {
    let mut items = Vec::new();
    for line in reply.lines() {
        let line = line.trim().trim_end_matches(',');
        let (Some(start), Some(end)) = (line.find('{'), line.rfind('}')) else {
            continue;
        };
        let Some(v) = line
            .get(start..=end)
            .and_then(|j| serde_json::from_str::<serde_json::Value>(j).ok())
        else {
            continue;
        };
        let field = |k: &str| {
            v.get(k)
                .and_then(|x| x.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
        };
        let Some(q) = field("q") else { continue };
        items.push(PracticeItem {
            id: items.len() as u32 + 1,
            text: q,
            speaker: None,
            start_ms: None,
            end_ms: None,
            is_me: false,
            is_question: true,
            reference: field("a"),
            category: field("c"),
            followups: Vec::new(),
        });
    }
    (!items.is_empty()).then(|| PracticeScript {
        kind: PracticeKind::Questions,
        title: title.to_string(),
        items,
        speakers: Vec::new(),
        me_speaker: None,
        needs_classification: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_request_carries_role_material_and_count() {
        let m = Material {
            name: "jd.md".into(),
            path: String::new(),
            text: "We need a network engineer who knows BGP.".into(),
        };
        let req = request("m", "Network engineer", &[m], 50, "English");
        let system = &req.system;
        assert!(system.contains("write 20 questions"), "count is capped");
        assert!(system.contains("Network engineer"));
        assert!(system.contains("knows BGP"));
        assert_eq!(req.purpose, UsagePurpose::Generate);
    }

    #[test]
    fn json_lines_become_questions() {
        let reply = "```json\n{\"q\":\"Tell me about yourself.\",\"a\":\"I build networks.\",\"c\":\"Opener\"}\nnot json\n{\"q\":\"How does BGP pick a route?\",\"c\":\"BGP\"},\n{\"a\":\"no question\"}\n```";
        let s = parse(reply, "Mock").unwrap();
        assert_eq!(s.items.len(), 2);
        assert_eq!(s.items[0].id, 1);
        assert_eq!(s.items[0].reference.as_deref(), Some("I build networks."));
        assert_eq!(s.items[1].category.as_deref(), Some("BGP"));
        assert!(s.items[1].reference.is_none());
        assert!(parse("nothing", "Mock").is_none());
    }
}
