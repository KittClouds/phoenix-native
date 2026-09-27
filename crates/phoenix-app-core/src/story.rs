//! Story timeline (4C): when each graph object first appears in reading
//! order, from stored spans only.
//!
//! A position is a byte offset in the verified generation's document
//! revision. Structure (chapters, paragraphs, sentences, passages, spans)
//! appears at its own start, evidence at its start, an entity at its earliest
//! evidence start, and an event at its stored span's start. The document node
//! is present from the beginning. Anything without a stored span (episodes,
//! events without a span, entities without evidence) is left out and shown
//! as untimed; it is never given a guessed time. Edges are not listed: an
//! edge appears once both of its ends have.

use phoenix_graph_generation_v2::{
    ChapterRecord, ChunkRecord, DocumentRecord, EventRecord, EvidenceRecord, PageKind,
    ParagraphRecord, SentenceRecord, SpanRecord, VerifiedGraphGenerationV2,
};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoryTimeline {
    /// Graph node id and first-appearance offset, sorted by offset then id.
    pub appearances: Arc<[(u64, u32)]>,
    /// Chapter start offsets in reading order, for scrubber ticks.
    pub chapters: Arc<[u32]>,
    /// One past the last byte of the document revision.
    pub end: u32,
    pub document_revision: u64,
}

impl StoryTimeline {
    /// Builds the timeline from verified pages; a page that cannot be read
    /// yields `None` rather than a partial timeline.
    #[must_use]
    pub fn build(generation: &VerifiedGraphGenerationV2) -> Option<Self> {
        let header = generation.header();
        let [document] = generation
            .typed_page::<DocumentRecord>(PageKind::Documents)
            .ok()?
        else {
            return None;
        };
        let chapters = generation
            .typed_page::<ChapterRecord>(PageKind::Chapters)
            .ok()?;
        let paragraphs = generation
            .typed_page::<ParagraphRecord>(PageKind::Paragraphs)
            .ok()?;
        let sentences = generation
            .typed_page::<SentenceRecord>(PageKind::Sentences)
            .ok()?;
        let chunks = generation.typed_page::<ChunkRecord>(PageKind::Chunks).ok()?;
        let spans = generation.typed_page::<SpanRecord>(PageKind::Spans).ok()?;
        let evidence = generation
            .typed_page::<EvidenceRecord>(PageKind::Evidence)
            .ok()?;
        let events = generation.typed_page::<EventRecord>(PageKind::Events).ok()?;
        Some(Self::from_records(
            document,
            header.document_revision,
            chapters,
            paragraphs,
            sentences,
            chunks,
            spans,
            evidence,
            events,
        ))
    }

    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn from_records(
        document: &DocumentRecord,
        document_revision: u64,
        chapters: &[ChapterRecord],
        paragraphs: &[ParagraphRecord],
        sentences: &[SentenceRecord],
        chunks: &[ChunkRecord],
        spans: &[SpanRecord],
        evidence: &[EvidenceRecord],
        events: &[EventRecord],
    ) -> Self {
        let mut first: HashMap<u64, u32> = HashMap::new();
        let mut appear = |id: u64, at: u32| {
            first
                .entry(id)
                .and_modify(|seen| *seen = (*seen).min(at))
                .or_insert(at);
        };
        appear(document.id, 0);
        let spanned = |start: u32, end: u32| start < end;
        for r in chapters.iter().filter(|r| spanned(r.start, r.end)) {
            appear(r.id, r.start);
        }
        for r in paragraphs.iter().filter(|r| spanned(r.start, r.end)) {
            appear(r.id, r.start);
        }
        for r in sentences.iter().filter(|r| spanned(r.start, r.end)) {
            appear(r.id, r.start);
        }
        for r in chunks.iter().filter(|r| spanned(r.start, r.end)) {
            appear(r.id, r.start);
        }
        for r in spans.iter().filter(|r| spanned(r.start, r.end)) {
            appear(r.id, r.start);
        }
        for r in evidence.iter().filter(|r| spanned(r.start, r.end)) {
            appear(r.id, r.start);
            appear(r.entity_id, r.start);
        }
        for r in events.iter().filter(|r| spanned(r.start, r.end)) {
            appear(r.id, r.start);
        }
        let mut appearances: Vec<(u64, u32)> = first.into_iter().collect();
        appearances.sort_unstable_by_key(|&(id, at)| (at, id));
        let mut chapter_starts: Vec<(u32, u32)> = chapters
            .iter()
            .filter(|r| spanned(r.start, r.end))
            .map(|r| (r.ordinal, r.start))
            .collect();
        chapter_starts.sort_unstable();
        let end = chapters
            .iter()
            .map(|r| r.end)
            .chain(chunks.iter().map(|r| r.end))
            .chain(std::iter::once(document.source_len))
            .max()
            .unwrap_or(0);
        Self {
            appearances: appearances.into(),
            chapters: chapter_starts.into_iter().map(|(_, start)| start).collect(),
            end,
            document_revision,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytemuck::Zeroable;

    #[test]
    fn appearances_come_only_from_stored_spans() {
        let mut document = DocumentRecord::zeroed();
        document.id = 1;
        document.source_len = 500;
        let mut chapter_two = ChapterRecord::zeroed();
        (chapter_two.id, chapter_two.start, chapter_two.end, chapter_two.ordinal) = (11, 250, 500, 1);
        let mut chapter_one = ChapterRecord::zeroed();
        (chapter_one.id, chapter_one.start, chapter_one.end, chapter_one.ordinal) = (10, 0, 250, 0);
        let mut chunk = ChunkRecord::zeroed();
        (chunk.id, chunk.start, chunk.end) = (20, 40, 120);
        let evidence = |id, entity, start, end| {
            let mut r = EvidenceRecord::zeroed();
            (r.id, r.entity_id, r.start, r.end) = (id, entity, start, end);
            r
        };
        let mut timed_event = EventRecord::zeroed();
        (timed_event.id, timed_event.start, timed_event.end) = (40, 300, 320);
        let mut untimed_event = EventRecord::zeroed();
        untimed_event.id = 41;
        let timeline = StoryTimeline::from_records(
            &document,
            2,
            &[chapter_two, chapter_one],
            &[],
            &[],
            &[chunk],
            &[],
            // Entity 30 first appears at its earliest evidence (60), not 400.
            &[evidence(31, 30, 400, 410), evidence(32, 30, 60, 70), evidence(33, 35, 5, 5)],
            &[timed_event, untimed_event],
        );
        let at = |id: u64| timeline.appearances.iter().find(|a| a.0 == id).map(|a| a.1);
        assert_eq!(at(1), Some(0));
        assert_eq!(at(11), Some(250));
        assert_eq!(at(20), Some(40));
        assert_eq!(at(30), Some(60));
        assert_eq!(at(40), Some(300));
        // No stored span: untimed, never guessed.
        assert_eq!(at(41), None);
        assert_eq!(at(33), None);
        assert_eq!(at(35), None);
        assert_eq!(&*timeline.chapters, &[0, 250]);
        assert_eq!(timeline.end, 500);
        assert!(timeline.appearances.windows(2).all(|w| w[0].1 <= w[1].1));
    }
}
