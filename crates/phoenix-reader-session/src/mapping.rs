use crate::{Error, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ByteRange {
    pub start: u32,
    pub end: u32,
}
impl ByteRange {
    pub fn slice(self, text: &str) -> Result<&str> {
        text.get(self.start as usize..self.end as usize)
            .ok_or(Error::Invalid("invalid UTF-8 range"))
    }
    pub fn contains(self, other: Self) -> bool {
        self.start <= other.start && other.end <= self.end
    }
    pub fn overlaps(self, other: Self) -> bool {
        self.start < other.end && other.start < self.end
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MappingKind {
    Copy,
    Replace,
    Omit,
    Insert,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MappingRun {
    pub source: ByteRange,
    pub spoken: ByteRange,
    pub kind: MappingKind,
    /// Zero only for Copy; otherwise indexes the plan's versioned rule table.
    pub rule: u32,
}

/// Complete ordered accounting, including markup omissions. No per-run allocation.
pub fn validate_mapping(
    source: &str,
    spoken: &str,
    runs: &[MappingRun],
    rule_count: usize,
) -> Result<()> {
    let (mut source_at, mut spoken_at) = (0, 0);
    for run in runs {
        if run.source.start != source_at || run.spoken.start != spoken_at {
            return Err(Error::Invalid("mapping gap, overlap, or reordering"));
        }
        let a = run.source.slice(source)?;
        let b = run.spoken.slice(spoken)?;
        let valid = match run.kind {
            MappingKind::Copy => !a.is_empty() && a == b && run.rule == 0,
            MappingKind::Replace => !a.is_empty() && !b.is_empty(),
            MappingKind::Omit => !a.is_empty() && b.is_empty(),
            MappingKind::Insert => a.is_empty() && !b.is_empty(),
        };
        if !valid
            || (run.kind != MappingKind::Copy && (run.rule == 0 || run.rule as usize > rule_count))
        {
            return Err(Error::Invalid("invalid mapping transformation"));
        }
        source_at = run.source.end;
        spoken_at = run.spoken.end;
    }
    if source_at as usize != source.len() || spoken_at as usize != spoken.len() {
        return Err(Error::Invalid("incomplete source or speech coverage"));
    }
    Ok(())
}

/// Emits full replacement ranges; callers apply grapheme-aware UI projection.
/// Inserts have no source paint. Copy intersections retain exact byte offsets.
pub struct SourceMap<'a> {
    source: &'a str,
    spoken: &'a str,
    runs: &'a [MappingRun],
}
impl<'a> SourceMap<'a> {
    pub fn new(
        source: &'a str,
        spoken: &'a str,
        runs: &'a [MappingRun],
        rule_count: usize,
    ) -> Result<Self> {
        validate_mapping(source, spoken, runs, rule_count)?;
        Ok(Self {
            source,
            spoken,
            runs,
        })
    }
    pub fn project(&self, query: ByteRange, emit: impl FnMut(ByteRange)) -> Result<()> {
        query.slice(self.spoken)?;
        if query.start == query.end {
            return Ok(());
        }
        project_spoken(self.runs, query, emit);
        Ok(())
    }
    pub fn source(&self) -> &str {
        self.source
    }
}
pub(crate) fn project_spoken(
    runs: &[MappingRun],
    query: ByteRange,
    mut emit: impl FnMut(ByteRange),
) {
    let start = runs.partition_point(|r| r.spoken.end <= query.start);
    for run in &runs[start..] {
        if run.spoken.start >= query.end {
            break;
        }
        if !run.spoken.overlaps(query) {
            continue;
        }
        match run.kind {
            MappingKind::Copy => emit(ByteRange {
                start: run.source.start + query.start.saturating_sub(run.spoken.start),
                end: run.source.start + query.end.min(run.spoken.end) - run.spoken.start,
            }),
            MappingKind::Replace => emit(run.source),
            MappingKind::Omit | MappingKind::Insert => {}
        }
    }
}

/// SIMD-dispatched memchr scanning, useful to a planner without allocating lines.
pub fn paragraph_breaks(source: &str) -> impl Iterator<Item = usize> + '_ {
    memchr::memmem::find_iter(source.as_bytes(), b"\n\n")
}
