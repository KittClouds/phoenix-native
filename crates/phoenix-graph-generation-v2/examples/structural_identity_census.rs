use hashbrown::HashSet;
use phoenix_graph_generation_v2::{
    ChapterRecord, DocumentRecord, PageKind, ParagraphRecord, SentenceRecord,
    VerifiedGraphGenerationV2, VerifiedTopologyV2,
};
use std::hash::Hash;
use std::path::Path;

fn duplicate_count<T: Copy + Eq + Hash>(values: impl Iterator<Item = T>, capacity: usize) -> usize {
    let mut seen = HashSet::with_capacity(capacity);
    values.filter(|value| !seen.insert(*value)).count()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os()
        .nth(1)
        .ok_or("usage: structural_identity_census <generation.pgg2>")?;
    let generation = VerifiedGraphGenerationV2::open(Path::new(&path))?;
    VerifiedTopologyV2::open(&generation)?;

    let documents: &[DocumentRecord] = generation.typed_page(PageKind::Documents)?;
    let chapters: &[ChapterRecord] = generation.typed_page(PageKind::Chapters)?;
    let paragraphs: &[ParagraphRecord] = generation.typed_page(PageKind::Paragraphs)?;
    let sentences: &[SentenceRecord] = generation.typed_page(PageKind::Sentences)?;
    let [document] = documents else {
        return Err(format!("expected one document, found {}", documents.len()).into());
    };

    let chapter_ids = HashSet::<u64>::from_iter(chapters.iter().map(|record| record.id));
    let paragraph_ids = HashSet::<u64>::from_iter(paragraphs.iter().map(|record| record.id));
    let chapter_parent_errors = paragraphs
        .iter()
        .filter(|record| !chapter_ids.contains(&record.chapter_id))
        .count();
    let paragraph_parent_errors = sentences
        .iter()
        .filter(|record| !paragraph_ids.contains(&record.paragraph_id))
        .count();
    let chapter_child_errors = chapters
        .iter()
        .filter(|chapter| {
            let range = chapter.paragraph_start as usize..chapter.paragraph_end as usize;
            paragraphs
                .get(range)
                .is_none_or(|children| children.iter().any(|child| child.chapter_id != chapter.id))
        })
        .count();
    let paragraph_child_errors = paragraphs
        .iter()
        .filter(|paragraph| {
            let range = paragraph.sentence_start as usize..paragraph.sentence_end as usize;
            sentences.get(range).is_none_or(|children| {
                children
                    .iter()
                    .any(|child| child.paragraph_id != paragraph.id)
            })
        })
        .count();

    let chapter_id_duplicates =
        duplicate_count(chapters.iter().map(|record| record.id), chapters.len());
    let chapter_ordinal_duplicates = duplicate_count(
        chapters
            .iter()
            .map(|record| (record.document_id, record.ordinal)),
        chapters.len(),
    );
    let chapter_span_duplicates = duplicate_count(
        chapters
            .iter()
            .map(|record| (record.document_id, record.start, record.end)),
        chapters.len(),
    );
    let paragraph_id_duplicates =
        duplicate_count(paragraphs.iter().map(|record| record.id), paragraphs.len());
    let paragraph_ordinal_duplicates = duplicate_count(
        paragraphs
            .iter()
            .map(|record| (record.document_id, record.ordinal)),
        paragraphs.len(),
    );
    let paragraph_span_duplicates = duplicate_count(
        paragraphs
            .iter()
            .map(|record| (record.document_id, record.start, record.end)),
        paragraphs.len(),
    );
    let sentence_id_duplicates =
        duplicate_count(sentences.iter().map(|record| record.id), sentences.len());
    let sentence_ordinal_duplicates = duplicate_count(
        sentences
            .iter()
            .map(|record| (record.document_id, record.ordinal)),
        sentences.len(),
    );
    let sentence_span_duplicates = duplicate_count(
        sentences
            .iter()
            .map(|record| (record.document_id, record.start, record.end)),
        sentences.len(),
    );
    let document_count_errors = usize::from(document.chapter_count as usize != chapters.len())
        + usize::from(document.paragraph_count as usize != paragraphs.len())
        + usize::from(document.sentence_count as usize != sentences.len());

    println!(
        "STRUCTURAL_IDENTITY_CENSUS generation={} revision={} chapters={} paragraphs={} sentences={}",
        generation.header().published_generation,
        generation.header().document_revision,
        chapters.len(),
        paragraphs.len(),
        sentences.len()
    );
    println!(
        "chapters id_duplicates={chapter_id_duplicates} ordinal_duplicates={chapter_ordinal_duplicates} span_duplicates={chapter_span_duplicates} child_range_errors={chapter_child_errors}"
    );
    println!(
        "paragraphs id_duplicates={paragraph_id_duplicates} ordinal_duplicates={paragraph_ordinal_duplicates} span_duplicates={paragraph_span_duplicates} parent_errors={chapter_parent_errors} child_range_errors={paragraph_child_errors}"
    );
    println!(
        "sentences id_duplicates={sentence_id_duplicates} ordinal_duplicates={sentence_ordinal_duplicates} span_duplicates={sentence_span_duplicates} parent_errors={paragraph_parent_errors}"
    );
    println!("document_count_errors={document_count_errors}");

    let errors = chapter_id_duplicates
        + chapter_ordinal_duplicates
        + chapter_span_duplicates
        + paragraph_id_duplicates
        + paragraph_ordinal_duplicates
        + paragraph_span_duplicates
        + sentence_id_duplicates
        + sentence_ordinal_duplicates
        + sentence_span_duplicates
        + chapter_parent_errors
        + paragraph_parent_errors
        + chapter_child_errors
        + paragraph_child_errors
        + document_count_errors;
    if errors != 0 {
        return Err(format!("structural identity census failed with {errors} errors").into());
    }
    Ok(())
}
