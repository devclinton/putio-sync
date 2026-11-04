//! `EVENTS.md` documents every event type with an example that matches the schema
//! (feature 027, T012)

use putio_sync_core::events::{validate_against_schema, Event, EventType};

const DOC: &str = include_str!("../../EVENTS.md");

/// The JSON blocks under each `### \`type\`` heading
fn sections() -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    let mut block: Option<String> = None;
    for line in DOC.lines() {
        if let Some(text) = block.as_mut() {
            if line.starts_with("```") {
                let done = block.take().unwrap();
                if let Some((_, blocks)) = out.last_mut() {
                    blocks.push(done);
                }
            } else {
                text.push_str(line);
                text.push('\n');
            }
        } else if line.starts_with("```json") {
            block = Some(String::new());
        } else if let Some(heading) = line.strip_prefix("### `") {
            out.push((heading.trim_end_matches('`').to_string(), Vec::new()));
        }
    }
    out
}

#[test]
fn every_event_type_has_a_section() {
    let headings: Vec<String> = sections().into_iter().map(|(name, _)| name).collect();
    let types: Vec<String> = EventType::ALL
        .iter()
        .map(|t| t.as_str().to_string())
        .collect();
    assert_eq!(headings, types);
}

#[test]
fn every_example_parses_and_matches_the_schema() {
    for (name, blocks) in sections() {
        assert_eq!(blocks.len(), 1, "{name} has one example");
        let value: serde_json::Value =
            serde_json::from_str(&blocks[0]).unwrap_or_else(|e| panic!("{name}: {e}"));
        validate_against_schema(&value).unwrap_or_else(|e| panic!("{name}: {e}"));
        let event: Event = serde_json::from_value(value).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(event.event_type().as_str(), name);
    }
}
