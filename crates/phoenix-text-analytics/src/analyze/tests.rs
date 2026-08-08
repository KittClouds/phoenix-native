use super::*;

#[test]
fn empty_document_is_explicit() {
    let result = analyze("  \n\n ");
    assert_eq!(result.word_count, 0);
    assert_eq!(result.reading_grade, "No text");
}

#[test]
fn analytics_cover_prose_health_and_all_lenses() {
    let source = "Ryan said the signal was golden. Ryan said the signal was golden. Ryan said it again. Ryan said it again. Ryan said it again. Ryan never noticed the thermal sensor without thinking. Suddenly the obsidian network shattered into blood and fire!";
    let result = analyze(source);
    assert_eq!(result.lenses.len(), LensKind::ALL.len());
    assert!(result.word_count > 30);
    assert!(result.flow_score <= 100);
    assert_eq!(result.sentence_spans.len(), result.sentence_count as usize);
    assert_eq!(result.source_hash, source_fingerprint(source));
    assert!(result
        .lens(LensKind::Echo)
        .unwrap()
        .items
        .iter()
        .any(|item| item.label == "ryan" && item.spans.len() == item.count as usize));
    let phrases = result.lens(LensKind::Phrases).unwrap();
    assert!(phrases.count > 0);
    assert!(phrases.items.iter().all(|item| !item.spans.is_empty()));
    assert!(result.lens(LensKind::Negation).unwrap().count >= 2);
    assert!(result.lens(LensKind::Diction).unwrap().count >= 4);
}

#[test]
fn sentence_distribution_is_complete() {
    let result = analyze("One. Two words. This sentence has exactly seven ordinary words here. This deliberately elaborate sentence contains more than sixteen words so it enters a longer and more interesting rhythm bucket.");
    assert_eq!(
        result
            .sentence_bands
            .iter()
            .map(|band| band.count)
            .sum::<u32>(),
        result.sentence_count
    );
    assert!((99..=101).contains(
        &result
            .sentence_bands
            .iter()
            .map(|band| band.percent as u32)
            .sum::<u32>()
    ));
}

#[test]
#[ignore = "explicit large-document performance gate"]
fn large_document_performance_gate() {
    let paragraph = "Ryan noticed the thermal signal, but Ryan never trusted the golden network. The city answered with a long and luminous mechanical whisper. ";
    let source = paragraph.repeat(2_000);
    let started = std::time::Instant::now();
    let result = analyze(&source);
    assert!(result.word_count > 30_000);
    assert!(
        started.elapsed() < std::time::Duration::from_millis(500),
        "analysis took {:?}",
        started.elapsed()
    );
}
