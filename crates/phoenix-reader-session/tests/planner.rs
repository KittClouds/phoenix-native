use phoenix_reader_session::{plan_markdown, DocumentBinding, PlannerConfig};
use phoenix_workspace::{ContentHash, DocumentLease, DocumentRevision, EntryId};
use std::sync::Arc;

fn lease(text: &str) -> DocumentLease {
    DocumentLease {
        entry_id: EntryId(77),
        revision: DocumentRevision(4),
        content_hash: ContentHash::of(text.as_bytes()),
        content: Arc::from(text),
    }
}

#[test]
fn plans_plain_markdown_with_headings_and_exact_source_binding() {
    let source = "# First\n\nHello world.\n\n## Second\n\nDr. Jones stays. Then leaves.\n";
    let result = plan_markdown([9; 32], &lease(source), PlannerConfig::default()).unwrap();
    assert_eq!(result.receipt.chapters, 2);
    // The two headings and the two prose sentences are narrated as five
    // sentence segments because the honorific-aware splitter keeps "Dr." with
    // the following name.
    assert_eq!(result.receipt.segments, 5);
    assert_eq!(result.receipt.source_bytes, source.len() as u32);
    assert_eq!(
        result.plan.spec().document,
        DocumentBinding {
            workspace: [9; 32],
            entry: 77,
            revision: 4,
            content: *blake3::hash(source.as_bytes()).as_bytes()
        }
    );
    assert!(result
        .plan
        .spec()
        .segments
        .iter()
        .all(|segment| segment.source.start < segment.source.end));
    for segment in result.plan.spec().segments.iter() {
        assert!(segment.spoken.slice(&result.plan.spec().spoken).is_ok());
        assert!(segment.source.slice(source).is_ok());
    }
}

#[test]
fn preserves_visible_text_and_accounts_for_markdown_omissions() {
    let source = "# Title\n\n**Bold** and [link](https://example.invalid).\n\n```text\nsecret\n```\n\n![cover](cover.png)\n";
    let result = plan_markdown([1; 32], &lease(source), PlannerConfig::default()).unwrap();
    let spoken = &result.plan.spec().spoken;
    assert!(spoken.contains("Bold"));
    assert!(spoken.contains("link"));
    assert!(!spoken.contains("secret"));
    assert!(!spoken.contains("cover.png"));
    assert!(result.receipt.code_blocks_omitted >= 1);
    assert!(result.receipt.images_omitted >= 1);
    assert!(result.receipt.omitted_bytes > 0);
}

#[test]
fn rejects_unsupported_inputs_and_invalid_limits_with_source_offset() {
    let table = "| a | b |\n| - | - |\n| c | d |\n";
    let error = plan_markdown([1; 32], &lease(table), PlannerConfig::default()).unwrap_err();
    assert!(error.to_string().contains("unsupported-table-or-footnote"));
    let html = "<div>no</div>\n";
    let error = plan_markdown([1; 32], &lease(html), PlannerConfig::default()).unwrap_err();
    assert!(error.to_string().contains("unsupported-raw-html"));
    let error = plan_markdown(
        [1; 32],
        &lease("text"),
        PlannerConfig {
            max_segment_bytes: 1,
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(error
        .to_string()
        .contains("planner document or configuration bounds"));
}

#[test]
fn splits_long_sentences_only_at_unicode_grapheme_word_boundaries() {
    let source = "A very long sentence with café and naïve words that should split cleanly without breaking graphemes.";
    let result = plan_markdown(
        [1; 32],
        &lease(source),
        PlannerConfig {
            max_segment_bytes: 64,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(result.plan.spec().segments.len() > 1);
    for segment in result.plan.spec().segments.iter() {
        let spoken = segment.spoken.slice(&result.plan.spec().spoken).unwrap();
        assert!(spoken.len() <= 64);
        assert!(!spoken.is_empty());
        assert!(spoken.is_char_boundary(spoken.len()));
    }
}
