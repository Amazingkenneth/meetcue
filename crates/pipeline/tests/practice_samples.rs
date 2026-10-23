//! The sample practice files in `samples/practice` parse and pace as the
//! documentation describes.

use meetcue_pipeline::practice::parse::parse_file;
use meetcue_pipeline::practice::schedule::{Pacer, PacerConfig};
use meetcue_pipeline::practice::PracticeKind;

fn sample(name: &str) -> Vec<u8> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../samples/practice/");
    std::fs::read(format!("{path}{name}")).expect("sample file")
}

#[test]
fn sdn_subtitles() {
    let s = parse_file("sdn-interview.srt", &sample("sdn-interview.srt")).unwrap();
    assert_eq!(s.kind, PracticeKind::Subtitles);
    assert_eq!(s.speakers, vec!["面试官", "候选人"]);
    assert_eq!(s.me_speaker.as_deref(), Some("候选人"));
    let p = Pacer::new(&s, PacerConfig::default());
    assert_eq!(p.total(), 3, "three answers to give");
}

#[test]
fn network_question_list() {
    let s = parse_file("questions-network.csv", &sample("questions-network.csv")).unwrap();
    assert_eq!(s.kind, PracticeKind::Questions);
    assert_eq!(s.items.len(), 10);
    assert!(s.items.iter().all(|i| i.reference.is_some()));
    assert_eq!(s.items[4].followups.len(), 2);
    assert_eq!(s.items[2].category.as_deref(), Some("Data center"));
}
