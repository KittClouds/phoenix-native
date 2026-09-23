mod render;
mod segments;
use crate::{Chapter, Digest, DocumentBinding, Error, NarrationPlan, PlanSpec, Result};
use phoenix_tts_contract::digest;
use phoenix_workspace::{DocumentLease, MAX_DOCUMENT_BYTES};
use serde::Serialize;

#[derive(Clone, Copy, Debug, Serialize)]
pub struct PlannerConfig {
    /// Headings at this level or above start chapters. Default: H1 and H2.
    pub chapter_level: u8,
    /// Transport/planner byte bound, NOT a model token budget.
    pub max_segment_bytes: u32,
}
impl Default for PlannerConfig {
    fn default() -> Self {
        Self {
            chapter_level: 2,
            max_segment_bytes: 2048,
        }
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct PlannerReceipt {
    pub plan_id: Digest,
    pub planner_id: Digest,
    pub source_bytes: u32,
    pub spoken_bytes: u32,
    pub chapters: u32,
    pub segments: u32,
    pub mapping_runs: u32,
    pub omitted_bytes: u32,
    pub code_blocks_omitted: u32,
    pub images_omitted: u32,
    pub metadata_blocks_omitted: u32,
}
#[derive(Debug)]
pub struct PlannedNarration {
    pub plan: NarrationPlan,
    pub receipt: PlannerReceipt,
}

/// Pure planning: no model, filesystem mutation, network, or editor operations.
/// English honorific hints supplement Unicode sentence boundaries. This is a
/// deterministic heuristic, not a claim of multilingual linguistic perfection.
pub fn plan_markdown(
    workspace: Digest,
    lease: &DocumentLease,
    config: PlannerConfig,
) -> Result<PlannedNarration> {
    if lease.content.len() > MAX_DOCUMENT_BYTES
        || !(1..=6).contains(&config.chapter_level)
        || !(64..=16_384).contains(&config.max_segment_bytes)
    {
        return Err(Error::Invalid("planner document or configuration bounds"));
    }
    let document = DocumentBinding::from_lease(workspace, lease)?;
    let rendered = render::markdown(&lease.content, config.chapter_level)?;
    let segments = segments::segment(&rendered, config.max_segment_bytes as usize)?;
    let planner_id = digest(
        b"phoenix.markdown-planner/v1",
        &(
            "pulldown-cmark/0.13.4",
            "unicode-segmentation/1.13.3",
            "honorifics/v1",
            config,
        ),
    )?;
    let omitted_bytes = rendered
        .runs
        .iter()
        .filter(|r| r.kind == crate::MappingKind::Omit)
        .map(|r| r.source.end - r.source.start)
        .sum();
    let counts = (rendered.code_blocks, rendered.images, rendered.metadata);
    let plan = NarrationPlan::new(
        &lease.content,
        PlanSpec {
            document,
            planner: planner_id,
            pronunciation: *blake3::hash(b"phoenix.pronunciation/identity-v1").as_bytes(),
            rules: render::rule_hashes(),
            spoken: rendered.spoken.into_boxed_str(),
            mappings: rendered.runs.into_boxed_slice(),
            chapters: rendered
                .chapters
                .into_iter()
                .map(|source| Chapter { source })
                .collect(),
            segments: segments.into_boxed_slice(),
        },
    )?;
    let spec = plan.spec();
    let receipt = PlannerReceipt {
        plan_id: plan.id(),
        planner_id,
        source_bytes: lease.content.len() as u32,
        spoken_bytes: spec.spoken.len() as u32,
        chapters: spec.chapters.len() as u32,
        segments: spec.segments.len() as u32,
        mapping_runs: spec.mappings.len() as u32,
        omitted_bytes,
        code_blocks_omitted: counts.0,
        images_omitted: counts.1,
        metadata_blocks_omitted: counts.2,
    };
    Ok(PlannedNarration { plan, receipt })
}
fn error(code: &'static str, offset: usize) -> Error {
    Error::Planning {
        code,
        source_offset: offset as u32,
    }
}
