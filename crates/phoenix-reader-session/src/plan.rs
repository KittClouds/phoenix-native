use crate::{validate_mapping, ByteRange, Digest, Error, MappingRun, Result};
use phoenix_tts_contract::{digest, AlignmentLevel};
use phoenix_workspace::{DocumentLease, MAX_DOCUMENT_BYTES};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentBinding {
    pub workspace: Digest,
    pub entry: u64,
    pub revision: u64,
    pub content: Digest,
}
impl DocumentBinding {
    pub fn from_lease(workspace: Digest, lease: &DocumentLease) -> Result<Self> {
        if workspace == [0; 32]
            || lease.entry_id.0 == 0
            || lease.revision.0 == 0
            || blake3::hash(lease.content.as_bytes()).as_bytes() != &lease.content_hash.0
        {
            return Err(Error::Invalid("uncommitted or corrupt document lease"));
        }
        Ok(Self {
            workspace,
            entry: lease.entry_id.0,
            revision: lease.revision.0,
            content: lease.content_hash.0,
        })
    }
    pub fn matches_editor(self, workspace: Digest, lease: &DocumentLease) -> bool {
        Self::from_lease(workspace, lease).is_ok_and(|binding| binding == self)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chapter {
    pub source: ByteRange,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Segment {
    pub chapter: u32,
    pub sentence: u32,
    pub source: ByteRange,
    pub spoken: ByteRange,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlanSpec {
    pub document: DocumentBinding,
    pub planner: Digest,
    pub pronunciation: Digest,
    pub rules: Box<[Digest]>,
    pub spoken: Box<str>,
    pub mappings: Box<[MappingRun]>,
    pub chapters: Box<[Chapter]>,
    pub segments: Box<[Segment]>,
}
#[derive(Clone, Debug)]
pub struct NarrationPlan {
    id: Digest,
    spec: PlanSpec,
}
impl NarrationPlan {
    pub fn new(source: &str, spec: PlanSpec) -> Result<Self> {
        if source.len() > MAX_DOCUMENT_BYTES
            || spec.spoken.len() > MAX_DOCUMENT_BYTES * 4
            || spec.document.workspace == [0; 32]
            || spec.document.entry == 0
            || spec.document.revision == 0
            || spec.planner == [0; 32]
            || spec.pronunciation == [0; 32]
            || spec.rules.contains(&[0; 32])
            || spec.rules.len() > 65_536
            || spec.mappings.len() > 1_000_000
            || spec.segments.is_empty()
            || spec.segments.len() > 262_144
            || blake3::hash(source.as_bytes()).as_bytes() != &spec.document.content
        {
            return Err(Error::Invalid("invalid plan identity or bounds"));
        }
        validate_mapping(source, &spec.spoken, &spec.mappings, spec.rules.len())?;
        let mut end = 0;
        for chapter in &spec.chapters {
            chapter.source.slice(source)?;
            if chapter.source.start != end || chapter.source.end == end {
                return Err(Error::Invalid("chapter coverage"));
            }
            end = chapter.source.end;
        }
        if end as usize != source.len() {
            return Err(Error::Invalid("chapter coverage"));
        }
        let (mut spoken_end, mut source_end, mut previous_sentence) = (0, 0, 0);
        for segment in &spec.segments {
            segment.source.slice(source)?;
            if segment.spoken.slice(&spec.spoken)?.trim().is_empty() {
                return Err(Error::Invalid("empty segment speech"));
            }
            if segment.spoken.start != spoken_end {
                return Err(Error::Invalid("segment speech gap or overlap"));
            }
            if segment.source.start < source_end {
                return Err(Error::Invalid("segment source out of order"));
            }
            if segment.source.start == segment.source.end {
                return Err(Error::Invalid("empty segment source"));
            }
            if segment.sentence < previous_sentence {
                return Err(Error::Invalid("segment sentence out of order"));
            }
            if !spec
                .chapters
                .get(segment.chapter as usize)
                .is_some_and(|c| c.source.contains(segment.source))
            {
                return Err(Error::Invalid("segment crosses chapter"));
            }
            // Search only overlapping runs rather than rescanning the whole map.
            let first = spec
                .mappings
                .partition_point(|r| r.spoken.end <= segment.spoken.start);
            for run in &spec.mappings[first..] {
                if run.spoken.start >= segment.spoken.end {
                    break;
                }
                if run.spoken.overlaps(segment.spoken) && !segment.source.contains(run.source) {
                    // Copy runs may span adjacent sentences; project the actual intersection.
                    if run.kind != crate::MappingKind::Copy {
                        return Err(Error::Invalid("replacement crosses segment"));
                    }
                    let mapped = ByteRange {
                        start: run.source.start
                            + segment.spoken.start.saturating_sub(run.spoken.start),
                        end: run.source.start + segment.spoken.end.min(run.spoken.end)
                            - run.spoken.start,
                    };
                    if !segment.source.contains(mapped) {
                        return Err(Error::Invalid("segment source mismatch"));
                    }
                }
            }
            spoken_end = segment.spoken.end;
            source_end = segment.source.end;
            previous_sentence = segment.sentence;
        }
        if spoken_end as usize != spec.spoken.len() {
            return Err(Error::Invalid("unplanned speech"));
        }
        let id = digest(b"phoenix.narration-plan/v1", &spec)?;
        Ok(Self { id, spec })
    }
    pub fn id(&self) -> Digest {
        self.id
    }
    pub fn spec(&self) -> &PlanSpec {
        &self.spec
    }
    pub fn project(&self, query: ByteRange, emit: impl FnMut(ByteRange)) -> Result<()> {
        query.slice(&self.spec.spoken)?;
        if query.start == query.end {
            return Ok(());
        }
        crate::mapping::project_spoken(&self.spec.mappings, query, emit);
        Ok(())
    }
    pub fn segment(&self, id: u32) -> Result<&Segment> {
        self.spec
            .segments
            .get(id as usize)
            .ok_or(Error::Invalid("unknown segment"))
    }
    pub fn encode(&self) -> Result<Vec<u8>> {
        Ok(postcard::to_allocvec(&self.spec)?)
    }
    pub fn decode(source: &str, bytes: &[u8], expected: Digest) -> Result<Self> {
        if bytes.len() > 128 * 1024 * 1024 {
            return Err(Error::Invalid("oversized plan"));
        }
        let plan = Self::new(source, postcard::from_bytes(bytes)?)?;
        if plan.id != expected {
            return Err(Error::Invalid("plan digest mismatch"));
        }
        Ok(plan)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlignedRange {
    pub spoken: ByteRange,
    pub first_frame: u64,
    pub end_frame: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Alignment {
    pub level: AlignmentLevel,
    pub provenance: Digest,
    pub audio_hash: Digest,
    pub spoken_hash: Digest,
    pub ranges: Box<[AlignedRange]>,
}
impl Alignment {
    pub fn validate(&self, spoken: &str, audio_hash: Digest, frames: u64) -> Result<()> {
        if frames == 0
            || self.audio_hash != audio_hash
            || self.provenance == [0; 32]
            || self.spoken_hash != *blake3::hash(spoken.as_bytes()).as_bytes()
            || self.ranges.is_empty()
            || self.ranges.len() > spoken.len().max(1)
        {
            return Err(Error::Invalid("alignment identity"));
        }
        let (mut text_end, mut frame_end) = (0, 0);
        for range in &self.ranges {
            if range.spoken.slice(spoken)?.is_empty()
                || range.spoken.start < text_end
                || range.first_frame < frame_end
                || range.end_frame <= range.first_frame
                || range.end_frame > frames
            {
                return Err(Error::Invalid("alignment intervals"));
            }
            if !spoken
                .get(text_end as usize..range.spoken.start as usize)
                .is_some_and(|s| s.trim().is_empty())
            {
                return Err(Error::Invalid("unaligned speech"));
            }
            text_end = range.spoken.end;
            frame_end = range.end_frame;
        }
        if !spoken[text_end as usize..].trim().is_empty() {
            return Err(Error::Invalid("unaligned suffix"));
        }
        if self.level == AlignmentLevel::Segment
            && (self.ranges.len() != 1 || self.ranges[0].first_frame != 0 || frame_end != frames)
        {
            return Err(Error::Invalid(
                "segment alignment must cover complete audio",
            ));
        }
        Ok(())
    }
}
