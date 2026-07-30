use hashbrown::HashSet;
use phoenix_graph_generation_v2::{
    ChapterRecord, ChunkRecord, DocumentRecord, PageKind, ParagraphRecord, SentenceRecord,
    SpanRecord, StructuralEdgeRecord, VerifiedGraphGenerationV2,
};
use thiserror::Error;

/// Borrowed compiler input over verified V2 pages.
///
/// There is deliberately no source-text field and no API for deriving
/// paragraphs or chunks. A compiler accepting this type can only consume the
/// identities and source coordinates published by the structural producer.
#[derive(Clone, Copy)]
pub struct VerifiedStructuralSource<'a> {
    generation: &'a VerifiedGraphGenerationV2,
    document: &'a DocumentRecord,
    chapters: &'a [ChapterRecord],
    paragraphs: &'a [ParagraphRecord],
    sentences: &'a [SentenceRecord],
    chunks: &'a [ChunkRecord],
    spans: &'a [SpanRecord],
    structural_edges: &'a [StructuralEdgeRecord],
}

impl<'a> VerifiedStructuralSource<'a> {
    pub fn open(generation: &'a VerifiedGraphGenerationV2) -> Result<Self, StructuralSourceError> {
        let documents = typed(generation, PageKind::Documents)?;
        let [document] = documents else {
            return Err(StructuralSourceError::DocumentCount {
                actual: documents.len(),
            });
        };
        let source = Self {
            generation,
            document,
            chapters: typed(generation, PageKind::Chapters)?,
            paragraphs: typed(generation, PageKind::Paragraphs)?,
            sentences: typed(generation, PageKind::Sentences)?,
            chunks: typed(generation, PageKind::Chunks)?,
            spans: typed(generation, PageKind::Spans)?,
            structural_edges: typed(generation, PageKind::StructuralEdges)?,
        };
        source.validate_counts()?;
        source.validate_authority()?;
        Ok(source)
    }

    pub fn generation(&self) -> &'a VerifiedGraphGenerationV2 {
        self.generation
    }

    pub fn document(&self) -> &'a DocumentRecord {
        self.document
    }

    pub fn chapters(&self) -> &'a [ChapterRecord] {
        self.chapters
    }

    pub fn paragraphs(&self) -> &'a [ParagraphRecord] {
        self.paragraphs
    }

    pub fn sentences(&self) -> &'a [SentenceRecord] {
        self.sentences
    }

    pub fn chunks(&self) -> &'a [ChunkRecord] {
        self.chunks
    }

    pub fn spans(&self) -> &'a [SpanRecord] {
        self.spans
    }

    pub fn structural_edges(&self) -> &'a [StructuralEdgeRecord] {
        self.structural_edges
    }

    fn validate_counts(&self) -> Result<(), StructuralSourceError> {
        let expected = [
            (
                PageKind::Chapters,
                self.document.chapter_count,
                self.chapters.len(),
            ),
            (
                PageKind::Paragraphs,
                self.document.paragraph_count,
                self.paragraphs.len(),
            ),
            (
                PageKind::Sentences,
                self.document.sentence_count,
                self.sentences.len(),
            ),
            (
                PageKind::Chunks,
                self.document.chunk_count,
                self.chunks.len(),
            ),
            (PageKind::Spans, self.document.span_count, self.spans.len()),
            (
                PageKind::StructuralEdges,
                self.document.structural_edge_count,
                self.structural_edges.len(),
            ),
        ];
        for (page, declared, actual) in expected {
            if declared as usize != actual {
                return Err(StructuralSourceError::CountMismatch {
                    page,
                    declared,
                    actual,
                });
            }
        }
        Ok(())
    }

    fn validate_authority(&self) -> Result<(), StructuralSourceError> {
        let document_id = self.document.id;
        if document_id == 0
            || self
                .chapters
                .iter()
                .any(|record| record.document_id != document_id)
            || self
                .paragraphs
                .iter()
                .any(|record| record.document_id != document_id)
            || self
                .sentences
                .iter()
                .any(|record| record.document_id != document_id)
            || self
                .chunks
                .iter()
                .any(|record| record.document_id != document_id)
            || self
                .spans
                .iter()
                .any(|record| record.document_id != document_id)
        {
            return Err(StructuralSourceError::AuthorityMismatch);
        }

        let chapter_ids: HashSet<u64> = self.chapters.iter().map(|record| record.id).collect();
        let paragraph_ids: HashSet<u64> = self.paragraphs.iter().map(|record| record.id).collect();
        let mut node_ids = HashSet::with_capacity(
            1 + self.chapters.len()
                + self.paragraphs.len()
                + self.sentences.len()
                + self.chunks.len(),
        );
        node_ids.insert(document_id);
        node_ids.extend(self.chapters.iter().map(|record| record.id));
        node_ids.extend(self.paragraphs.iter().map(|record| record.id));
        node_ids.extend(self.sentences.iter().map(|record| record.id));
        node_ids.extend(self.chunks.iter().map(|record| record.id));
        let expected_node_count = 1
            + self.chapters.len()
            + self.paragraphs.len()
            + self.sentences.len()
            + self.chunks.len();
        let edge_ids: HashSet<u64> = self
            .structural_edges
            .iter()
            .map(|record| record.id)
            .collect();
        if chapter_ids.len() != self.chapters.len()
            || paragraph_ids.len() != self.paragraphs.len()
            || node_ids.len() != expected_node_count
            || edge_ids.len() != self.structural_edges.len()
            || self
                .paragraphs
                .iter()
                .any(|record| !chapter_ids.contains(&record.chapter_id))
            || self
                .sentences
                .iter()
                .any(|record| !paragraph_ids.contains(&record.paragraph_id))
            || self.structural_edges.iter().any(|record| {
                record.id == 0
                    || !node_ids.contains(&record.source_id)
                    || !node_ids.contains(&record.target_id)
            })
        {
            return Err(StructuralSourceError::AuthorityMismatch);
        }
        Ok(())
    }
}

fn typed<T: bytemuck::Pod>(
    generation: &VerifiedGraphGenerationV2,
    page: PageKind,
) -> Result<&[T], StructuralSourceError> {
    generation
        .typed_page(page)
        .map_err(|_| StructuralSourceError::InvalidPage(page))
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum StructuralSourceError {
    #[error("V2 structural page {0:?} has an invalid packed layout")]
    InvalidPage(PageKind),
    #[error("V2 structural authority contains {actual} document rows; expected exactly one")]
    DocumentCount { actual: usize },
    #[error("V2 structural page {page:?} declares {declared} rows but contains {actual}")]
    CountMismatch {
        page: PageKind,
        declared: u32,
        actual: usize,
    },
    #[error("V2 structural parent or document authority is inconsistent")]
    AuthorityMismatch,
}
