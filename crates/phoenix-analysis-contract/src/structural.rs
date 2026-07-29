use crate::{types::validate_binding, DocumentAnalysisBinding, MAX_TEXT_BYTES};
use serde::{Deserialize, Serialize};

pub const STRUCTURAL_SUBSTRATE_CONTRACT: &str = "phoenix.native.structural-substrate/v1";
pub const STRUCTURAL_ARTIFACT_EXTENSION: &str = "pnss";
pub const MAX_STRUCTURAL_RECORDS: usize = 2_000_000;
pub const NO_STRUCTURAL_PARENT: u32 = u32::MAX;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[repr(u16)]
pub enum StructuralDialogueHint {
    None = 0,
    OpensQuote = 1,
    ClosesQuote = 2,
    QuotedSentence = 3,
    DialogueLine = 4,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[repr(u16)]
pub enum StructuralSentenceQuality {
    Empty = 0,
    Fragment = 1,
    Complete = 2,
    RunOn = 3,
    NoTerminalPunctuation = 4,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[repr(u16)]
pub enum StructuralSpanKind {
    Paragraph = 1,
    Chapter = 2,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AnalysisChunkRecord {
    pub start: u32,
    pub end: u32,
    pub sentence_start: u32,
    pub sentence_end: u32,
    pub paragraph_start: u32,
    pub paragraph_end: u32,
    pub chapter_index: u32,
    pub token_count: u32,
    pub content_hash: u64,
    pub dialogue_hint: StructuralDialogueHint,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AnalysisSentenceRecord {
    pub start: u32,
    pub end: u32,
    pub paragraph_index: u32,
    pub chapter_index: u32,
    pub token_count: u32,
    pub content_hash: u64,
    pub quality: StructuralSentenceQuality,
    pub dialogue_hint: StructuralDialogueHint,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AnalysisSpanRecord {
    pub kind: StructuralSpanKind,
    pub start: u32,
    pub end: u32,
    pub parent_index: u32,
    pub child_start: u32,
    pub child_end: u32,
    pub token_count: u32,
    pub content_hash: u64,
    pub label: String,
    pub dialogue_hint: StructuralDialogueHint,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PhoenixStructuralSubstrateV1 {
    pub schema: String,
    pub binding: DocumentAnalysisBinding,
    pub source_len: u32,
    pub chunks: Vec<AnalysisChunkRecord>,
    pub sentences: Vec<AnalysisSentenceRecord>,
    pub spans: Vec<AnalysisSpanRecord>,
}

impl PhoenixStructuralSubstrateV1 {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != STRUCTURAL_SUBSTRATE_CONTRACT {
            return Err("unsupported structural substrate schema");
        }
        validate_binding(&self.binding)?;
        if self.source_len == 0 || self.source_len as usize > MAX_TEXT_BYTES {
            return Err("structural substrate source length is invalid");
        }
        if self.chunks.is_empty()
            || self.sentences.is_empty()
            || self.chunks.len() > MAX_STRUCTURAL_RECORDS
            || self.sentences.len() > MAX_STRUCTURAL_RECORDS
            || self.spans.len() > MAX_STRUCTURAL_RECORDS
        {
            return Err("structural substrate counts are invalid");
        }
        validate_ranges(
            self.source_len,
            self.chunks.iter().map(|record| (record.start, record.end)),
        )?;
        validate_ranges(
            self.source_len,
            self.sentences
                .iter()
                .map(|record| (record.start, record.end)),
        )?;
        validate_ranges(
            self.source_len,
            self.spans
                .iter()
                .filter(|record| record.kind == StructuralSpanKind::Paragraph)
                .map(|record| (record.start, record.end)),
        )?;
        validate_ranges(
            self.source_len,
            self.spans
                .iter()
                .filter(|record| record.kind == StructuralSpanKind::Chapter)
                .map(|record| (record.start, record.end)),
        )?;
        validate_chunks(self)?;
        validate_sentences(self)?;
        validate_spans(self)
    }
}

fn validate_ranges(
    source_len: u32,
    ranges: impl Iterator<Item = (u32, u32)>,
) -> Result<(), &'static str> {
    let mut previous = (0, 0);
    for (index, range) in ranges.enumerate() {
        if range.0 >= range.1
            || range.1 > source_len
            || (index > 0 && (range.0, range.1) < previous)
        {
            return Err("structural record range is invalid or unsorted");
        }
        previous = range;
    }
    Ok(())
}

fn validate_chunks(substrate: &PhoenixStructuralSubstrateV1) -> Result<(), &'static str> {
    let paragraph_count = substrate
        .spans
        .iter()
        .filter(|span| span.kind == StructuralSpanKind::Paragraph)
        .count();
    let chapter_count = substrate
        .spans
        .iter()
        .filter(|span| span.kind == StructuralSpanKind::Chapter)
        .count();
    for chunk in &substrate.chunks {
        if chunk.sentence_start > chunk.sentence_end
            || chunk.sentence_end as usize > substrate.sentences.len()
            || chunk.paragraph_start > chunk.paragraph_end
            || chunk.paragraph_end as usize > paragraph_count
            || (chunk.chapter_index != NO_STRUCTURAL_PARENT
                && chunk.chapter_index as usize >= chapter_count)
        {
            return Err("dynamic chunk membership is invalid");
        }
    }
    Ok(())
}

fn validate_sentences(substrate: &PhoenixStructuralSubstrateV1) -> Result<(), &'static str> {
    let paragraph_count = substrate
        .spans
        .iter()
        .filter(|span| span.kind == StructuralSpanKind::Paragraph)
        .count();
    let chapter_count = substrate
        .spans
        .iter()
        .filter(|span| span.kind == StructuralSpanKind::Chapter)
        .count();
    if substrate.sentences.iter().any(|sentence| {
        sentence.paragraph_index as usize >= paragraph_count
            || sentence.chapter_index as usize >= chapter_count
    }) {
        return Err("sentence structural membership is invalid");
    }
    Ok(())
}

fn validate_spans(substrate: &PhoenixStructuralSubstrateV1) -> Result<(), &'static str> {
    let paragraph_count = substrate
        .spans
        .iter()
        .filter(|span| span.kind == StructuralSpanKind::Paragraph)
        .count();
    let chapter_count = substrate.spans.len().saturating_sub(paragraph_count);
    for span in &substrate.spans {
        match span.kind {
            StructuralSpanKind::Paragraph => {
                if span.parent_index as usize >= chapter_count
                    || span.child_start > span.child_end
                    || span.child_end as usize > substrate.sentences.len()
                    || !span.label.is_empty()
                {
                    return Err("paragraph span membership is invalid");
                }
            }
            StructuralSpanKind::Chapter => {
                if span.parent_index != NO_STRUCTURAL_PARENT
                    || span.child_start > span.child_end
                    || span.child_end as usize > paragraph_count
                    || span.label.trim().is_empty()
                {
                    return Err("chapter span membership is invalid");
                }
            }
        }
    }
    Ok(())
}
