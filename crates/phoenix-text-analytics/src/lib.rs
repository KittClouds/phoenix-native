//! Allocation-conscious, deterministic prose analytics for Phoenix documents.
//!
//! The analyzer owns no source text after construction. It tokenizes once into
//! compact values, performs linear passes, and publishes a small immutable UI
//! snapshot. No analysis work belongs in a render loop.

mod analyze;

pub use analyze::{analyze, source_fingerprint};
use compact_str::CompactString;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum LensKind {
    Echo,
    Phrases,
    Proximity,
    Cadence,
    Negation,
    Ornament,
    Distance,
    Diction,
}

impl LensKind {
    pub const ALL: [Self; 8] = [
        Self::Echo,
        Self::Phrases,
        Self::Proximity,
        Self::Cadence,
        Self::Negation,
        Self::Ornament,
        Self::Distance,
        Self::Diction,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Echo => "Echo",
            Self::Phrases => "Phrases",
            Self::Proximity => "Proximity",
            Self::Cadence => "Cadence",
            Self::Negation => "Negation",
            Self::Ornament => "Ornament",
            Self::Distance => "Distance",
            Self::Diction => "Diction",
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::Echo => "repeated words and local pressure",
            Self::Phrases => "repeated multi-word phrases",
            Self::Proximity => "nearby repeated roots",
            Self::Cadence => "sentence rhythm hotspots",
            Self::Negation => "no / not / never / without frames",
            Self::Ornament => "lush or overloaded prose",
            Self::Distance => "felt / seemed / noticed filters",
            Self::Diction => "register shifts and texture bands",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceSpan {
    pub start: u32,
    pub end: u32,
}

impl SourceSpan {
    pub const fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RankedItem {
    pub label: CompactString,
    pub count: u32,
    pub detail: CompactString,
    pub spans: Box<[SourceSpan]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LensSummary {
    pub kind: LensKind,
    pub count: u32,
    pub items: Box<[RankedItem]>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SentenceBand {
    pub label: &'static str,
    pub count: u32,
    pub percent: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SentenceSpan {
    pub band: u8,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextAnalytics {
    pub source_hash: u64,
    pub word_count: u32,
    pub character_count: u32,
    pub sentence_count: u32,
    pub paragraph_count: u32,
    pub average_sentence_length: f32,
    pub reading_grade: CompactString,
    pub reading_seconds: u32,
    pub speaking_seconds: u32,
    pub flow_score: u8,
    pub variety_score: u8,
    pub has_monotony: bool,
    pub longest_monotony_run: u32,
    pub sentence_bands: [SentenceBand; 6],
    pub sentence_spans: Box<[SentenceSpan]>,
    pub lenses: Box<[LensSummary]>,
}

impl Default for TextAnalytics {
    fn default() -> Self {
        Self {
            source_hash: source_fingerprint(""),
            word_count: 0,
            character_count: 0,
            sentence_count: 0,
            paragraph_count: 0,
            average_sentence_length: 0.0,
            reading_grade: "No text".into(),
            reading_seconds: 0,
            speaking_seconds: 0,
            flow_score: 0,
            variety_score: 0,
            has_monotony: false,
            longest_monotony_run: 0,
            sentence_bands: [
                SentenceBand {
                    label: "1 word",
                    count: 0,
                    percent: 0,
                },
                SentenceBand {
                    label: "2-6 words",
                    count: 0,
                    percent: 0,
                },
                SentenceBand {
                    label: "7-15 words",
                    count: 0,
                    percent: 0,
                },
                SentenceBand {
                    label: "16-25 words",
                    count: 0,
                    percent: 0,
                },
                SentenceBand {
                    label: "26-39 words",
                    count: 0,
                    percent: 0,
                },
                SentenceBand {
                    label: "40+ words",
                    count: 0,
                    percent: 0,
                },
            ],
            sentence_spans: Box::new([]),
            lenses: Box::new([]),
        }
    }
}

impl TextAnalytics {
    pub fn lens(&self, kind: LensKind) -> Option<&LensSummary> {
        self.lenses.iter().find(|lens| lens.kind == kind)
    }
}
