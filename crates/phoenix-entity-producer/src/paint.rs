use crate::types::MentionAuthority;
use phoenix_graph_generation_v2::{EntityId, EvidenceId};

const SOURCE_USER_TAGGED: u16 = 1 << 1;
const MENTION_FLAG_ACCEPTED_EVIDENCE: u32 = 1 << 2;

/// Disposable editor projection. It is deliberately not a graph-authority
/// page: removing a span here can never remove a mention or evidence record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EditorPaintSpan {
    pub evidence_id: EvidenceId,
    pub entity_id: EntityId,
    pub start: u32,
    pub end: u32,
    pub kind: u16,
    pub source_mask: u16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EditorPaintProjection {
    pub spans: Box<[EditorPaintSpan]>,
    pub hash: [u8; 32],
}

pub(crate) fn build_editor_paint_projection(
    authority: &[MentionAuthority],
) -> EditorPaintProjection {
    let mut manual = authority
        .iter()
        .copied()
        .filter(|mention| mention.source_mask & SOURCE_USER_TAGGED != 0)
        .collect::<Vec<_>>();
    let mut ner = authority
        .iter()
        .copied()
        .filter(|mention| mention.source_mask & SOURCE_USER_TAGGED == 0)
        .collect::<Vec<_>>();
    sort_paint_candidates(&mut manual);
    sort_paint_candidates(&mut ner);

    let mut selected = Vec::with_capacity(authority.len());
    select_non_overlapping(&manual, &mut selected);
    select_non_overlapping(&ner, &mut selected);
    selected.sort_unstable_by_key(|span| (span.start, span.end, span.evidence_id.0));

    let spans = selected
        .into_iter()
        .map(|mention| EditorPaintSpan {
            evidence_id: mention.evidence_id,
            entity_id: mention.entity_id,
            start: mention.start,
            end: mention.end,
            kind: mention.kind,
            source_mask: mention.source_mask,
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let hash = paint_hash(&spans);
    EditorPaintProjection { spans, hash }
}

fn sort_paint_candidates(candidates: &mut [MentionAuthority]) {
    candidates.sort_unstable_by(|left, right| {
        left.start
            .cmp(&right.start)
            .then_with(|| (right.end - right.start).cmp(&(left.end - left.start)))
            .then_with(|| {
                (right.flags & MENTION_FLAG_ACCEPTED_EVIDENCE)
                    .cmp(&(left.flags & MENTION_FLAG_ACCEPTED_EVIDENCE))
            })
            .then_with(|| right.confidence_bits.cmp(&left.confidence_bits))
            .then_with(|| left.evidence_id.cmp(&right.evidence_id))
    });
}

fn select_non_overlapping(candidates: &[MentionAuthority], selected: &mut Vec<MentionAuthority>) {
    for candidate in candidates {
        if selected.iter().all(|current| !overlaps(candidate, current)) {
            selected.push(*candidate);
        }
    }
}

fn overlaps(left: &MentionAuthority, right: &MentionAuthority) -> bool {
    left.start < right.end && right.start < left.end
}

fn paint_hash(spans: &[EditorPaintSpan]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"phoenix.entity-producer/v1/editor-paint\0");
    for span in spans {
        hasher.update(&span.evidence_id.0.to_le_bytes());
        hasher.update(&span.entity_id.0.to_le_bytes());
        hasher.update(&span.start.to_le_bytes());
        hasher.update(&span.end.to_le_bytes());
        hasher.update(&span.kind.to_le_bytes());
        hasher.update(&span.source_mask.to_le_bytes());
    }
    *hasher.finalize().as_bytes()
}
