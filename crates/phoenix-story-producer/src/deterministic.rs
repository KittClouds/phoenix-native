use crate::{
    derive_causal_candidate_id, derive_episode_id, derive_event_id, derive_memory_candidate_id,
    derive_relationship_candidate_id, derive_temporal_candidate_id, publish_story_generation_new,
    CausalCandidateInput, CausalRelation, EpisodeCandidateInput, EpisodeFamily, EpisodeMember,
    EpisodeMembershipInput, EventCandidateInput, EventKind, MemoryStateCandidateInput,
    MemoryStateKind, ProducerRegistration, RelationshipCandidateInput, RelationshipKind,
    SemanticEndpoint, StoryProducerError, StoryProducerInput, StoryRegistrations,
    TemporalCandidateInput, TemporalRelation, VerifiedStoryGeneration,
};
use hashbrown::{HashMap, HashSet};
use phoenix_graph_generation_v2::{
    CandidateId, ChunkId, ChunkRecord, ContextualEvidenceRecord, EntityId, EventId, EvidenceId,
    EvidenceRecord, MentionRecord, PageKind, SentenceRecord, VerifiedGraphGenerationV2,
};
use std::path::Path;

pub const RELATIONSHIP_PRODUCER: &str = "phoenix-deterministic/relationship-v2";
pub const EVENT_PRODUCER: &str = "phoenix-deterministic/event-v2";
pub const EPISODE_PRODUCER: &str = "phoenix-deterministic/episode-v2";
pub const TEMPORAL_PRODUCER: &str = "phoenix-deterministic/temporal-v2";
pub const CAUSAL_PRODUCER: &str = "phoenix-deterministic/causal-v2";
pub const MEMORY_PRODUCER: &str = "phoenix-deterministic/memory-v2";

pub struct DeterministicStoryProducerInput<'a> {
    pub text: &'a str,
    pub source: &'a VerifiedGraphGenerationV2,
    pub contextual_evidence: &'a [ContextualEvidenceRecord],
    pub producer_binary_hash: [u8; 32],
    pub published_generation: u64,
}

#[derive(Clone)]
struct EventSpec {
    start: u32,
    end: u32,
    kind: EventKind,
    chunk_id: ChunkId,
    evidence: Vec<EvidenceId>,
    entities: Vec<EntityId>,
}

#[derive(Clone, Copy)]
struct RelationshipSpec {
    source: EntityId,
    target: EntityId,
    source_evidence: EvidenceId,
    target_evidence: EvidenceId,
    relation: RelationshipKind,
    confidence: f32,
}

pub fn publish_deterministic_story_generation_new(
    path: impl AsRef<Path>,
    input: DeterministicStoryProducerInput<'_>,
) -> Result<VerifiedStoryGeneration, StoryProducerError> {
    let mentions: &[MentionRecord] = input.source.typed_page(PageKind::Mentions)?;
    let evidence: &[EvidenceRecord] = input.source.typed_page(PageKind::Evidence)?;
    let chunks: &[ChunkRecord] = input.source.typed_page(PageKind::Chunks)?;
    let sentences: &[SentenceRecord] = input.source.typed_page(PageKind::Sentences)?;
    let evidence_by_id = evidence
        .iter()
        .map(|row| (EvidenceId(row.id), row))
        .collect::<HashMap<_, _>>();
    let mentions_by_chunk = mentions_grouped_by_chunk(mentions);

    let relationship_specs =
        relationship_specs(input.text, chunks, &mentions_by_chunk, &evidence_by_id);
    let mut relationships = relationship_specs
        .iter()
        .map(|spec| RelationshipCandidateInput {
            candidate_id: CandidateId::ZERO,
            source_entity_id: spec.source,
            target_entity_id: spec.target,
            source_evidence_id: spec.source_evidence,
            target_evidence_id: spec.target_evidence,
            additional_evidence_ids: &[],
            relation: spec.relation,
            confidence: spec.confidence,
        })
        .collect::<Vec<_>>();
    for row in &mut relationships {
        row.candidate_id = derive_relationship_candidate_id(
            &input.source.header().content_hash,
            RELATIONSHIP_PRODUCER,
            row,
        );
    }

    let event_specs = event_specs(input.text, sentences, chunks, evidence);
    let mut events = event_specs
        .iter()
        .map(|spec| EventCandidateInput {
            event_id: EventId(0),
            label: source_slice(input.text, spec.start, spec.end),
            label_start: spec.start,
            label_end: spec.end,
            evidence_ids: &spec.evidence,
            kind: spec.kind,
            confidence: 0.82,
        })
        .collect::<Vec<_>>();
    for row in &mut events {
        row.event_id = derive_event_id(&input.source.header().content_hash, EVENT_PRODUCER, row);
    }

    let episode_groups = episode_groups(&event_specs);
    let episode_evidence = episode_groups
        .iter()
        .map(|members| merged_evidence(members.iter().map(|index| &event_specs[*index].evidence)))
        .collect::<Vec<_>>();
    let episode_memberships = episode_groups
        .iter()
        .map(|members| {
            members
                .iter()
                .map(|index| EpisodeMembershipInput {
                    member: EpisodeMember::Event(events[*index].event_id),
                    evidence_ids: &event_specs[*index].evidence,
                    confidence: 0.84,
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let mut episodes = episode_groups
        .iter()
        .enumerate()
        .map(|(ordinal, members)| {
            let first = &event_specs[members[0]];
            let family = episode_family(members.iter().map(|index| {
                source_slice(
                    input.text,
                    event_specs[*index].start,
                    event_specs[*index].end,
                )
            }));
            EpisodeCandidateInput {
                episode_id: phoenix_graph_generation_v2::EpisodeId(0),
                label: source_slice(input.text, first.start, first.end),
                label_start: first.start,
                label_end: first.end,
                evidence_ids: &episode_evidence[ordinal],
                memberships: &episode_memberships[ordinal],
                ordinal: ordinal as u32,
                family,
                confidence: 0.78,
            }
        })
        .collect::<Vec<_>>();
    for row in &mut episodes {
        row.episode_id =
            derive_episode_id(&input.source.header().content_hash, EPISODE_PRODUCER, row);
    }

    let temporal_evidence = consecutive_event_evidence(&event_specs);
    let mut temporal = temporal_evidence
        .iter()
        .map(|(left, right, evidence)| TemporalCandidateInput {
            candidate_id: CandidateId::ZERO,
            source: SemanticEndpoint::Event(events[*left].event_id),
            target: SemanticEndpoint::Event(events[*right].event_id),
            evidence_ids: evidence,
            relation: TemporalRelation::Before,
            confidence: 0.88,
        })
        .collect::<Vec<_>>();
    for row in &mut temporal {
        row.candidate_id = derive_temporal_candidate_id(
            &input.source.header().content_hash,
            TEMPORAL_PRODUCER,
            row,
        );
    }

    let causal_pairs = causal_pairs(input.text, &event_specs);
    let mut causal = causal_pairs
        .iter()
        .map(
            |(left, right, evidence, relation, confidence)| CausalCandidateInput {
                candidate_id: CandidateId::ZERO,
                cause: SemanticEndpoint::Event(events[*left].event_id),
                effect: SemanticEndpoint::Event(events[*right].event_id),
                evidence_ids: evidence,
                relation: *relation,
                confidence: *confidence,
            },
        )
        .collect::<Vec<_>>();
    for row in &mut causal {
        row.candidate_id =
            derive_causal_candidate_id(&input.source.header().content_hash, CAUSAL_PRODUCER, row);
    }

    let memory_specs = memory_specs(input.text, &event_specs);
    let mut memory = memory_specs
        .iter()
        .map(
            |(event_index, entity, evidence, kind, key, value)| MemoryStateCandidateInput {
                candidate_id: CandidateId::ZERO,
                subject_entity_id: *entity,
                context: SemanticEndpoint::Event(events[*event_index].event_id),
                key,
                value,
                evidence_ids: evidence,
                kind: *kind,
                confidence: 0.76,
            },
        )
        .collect::<Vec<_>>();
    for row in &mut memory {
        row.candidate_id =
            derive_memory_candidate_id(&input.source.header().content_hash, MEMORY_PRODUCER, row);
    }

    publish_story_generation_new(
        path,
        StoryProducerInput {
            text: input.text,
            source: input.source,
            registrations: StoryRegistrations {
                relationships: deterministic(RELATIONSHIP_PRODUCER, &relationships),
                events: deterministic(EVENT_PRODUCER, &events),
                episodes: deterministic(EPISODE_PRODUCER, &episodes),
                temporal: deterministic(TEMPORAL_PRODUCER, &temporal),
                causal: deterministic(CAUSAL_PRODUCER, &causal),
                memory_state: deterministic(MEMORY_PRODUCER, &memory),
            },
            contextual_evidence: input.contextual_evidence,
            model_ranking: None,
            producer_binary_hash: input.producer_binary_hash,
            published_generation: input.published_generation,
        },
    )
}

fn deterministic<'a, T>(producer_id: &'a str, rules: &'a [T]) -> ProducerRegistration<'a, T> {
    ProducerRegistration::Deterministic { producer_id, rules }
}

fn mentions_grouped_by_chunk(mentions: &[MentionRecord]) -> HashMap<ChunkId, Vec<&MentionRecord>> {
    let mut groups = HashMap::<ChunkId, Vec<&MentionRecord>>::new();
    for mention in mentions {
        groups
            .entry(ChunkId(mention.chunk_id))
            .or_default()
            .push(mention);
    }
    for rows in groups.values_mut() {
        rows.sort_unstable_by_key(|row| (row.start, row.end, row.id));
    }
    groups
}

fn relationship_specs(
    text: &str,
    chunks: &[ChunkRecord],
    groups: &HashMap<ChunkId, Vec<&MentionRecord>>,
    evidence: &HashMap<EvidenceId, &EvidenceRecord>,
) -> Vec<RelationshipSpec> {
    let mut output = Vec::new();
    let mut unique = HashSet::new();
    for chunk in chunks {
        let Some(rows) = groups.get(&ChunkId(chunk.id)) else {
            continue;
        };
        let lower = source_slice(text, chunk.start, chunk.end).to_ascii_lowercase();
        let Some((relation, confidence)) = relationship_kind(&lower) else {
            continue;
        };
        for pair in rows.windows(2) {
            let left = pair[0];
            let right = pair[1];
            let distance = right.start.saturating_sub(left.end);
            if left.entity_id == right.entity_id || distance > 260 {
                continue;
            }
            let source_evidence = EvidenceId(left.evidence_id);
            let target_evidence = EvidenceId(right.evidence_id);
            if !evidence.contains_key(&source_evidence) || !evidence.contains_key(&target_evidence)
            {
                continue;
            }
            let key = (
                left.entity_id,
                right.entity_id,
                relation as u16,
                left.id,
                right.id,
            );
            if unique.insert(key) {
                output.push(RelationshipSpec {
                    source: EntityId(left.entity_id),
                    target: EntityId(right.entity_id),
                    source_evidence,
                    target_evidence,
                    relation,
                    confidence,
                });
            }
        }
    }
    output
}

fn event_specs(
    text: &str,
    sentences: &[SentenceRecord],
    chunks: &[ChunkRecord],
    evidence: &[EvidenceRecord],
) -> Vec<EventSpec> {
    let mut output = Vec::new();
    for sentence in sentences {
        let (start, end) = trim_range(text, sentence.start, sentence.end);
        if start >= end {
            continue;
        }
        let label = source_slice(text, start, end);
        let Some(kind) = event_kind(&label.to_ascii_lowercase()) else {
            continue;
        };
        let mut rows = evidence
            .iter()
            .filter(|row| row.start < end && row.end > start)
            .collect::<Vec<_>>();
        rows.sort_unstable_by_key(|row| row.id);
        rows.dedup_by_key(|row| row.id);
        if rows.is_empty() {
            continue;
        }
        let chunk_id = rows
            .first()
            .map(|row| ChunkId(row.chunk_id))
            .or_else(|| {
                chunks
                    .iter()
                    .find(|chunk| chunk.start < end && chunk.end > start)
                    .map(|row| ChunkId(row.id))
            })
            .unwrap_or(ChunkId(0));
        let mut entities = rows
            .iter()
            .map(|row| EntityId(row.entity_id))
            .collect::<Vec<_>>();
        entities.sort_unstable();
        entities.dedup();
        output.push(EventSpec {
            start,
            end,
            kind,
            chunk_id,
            evidence: rows.iter().map(|row| EvidenceId(row.id)).collect(),
            entities,
        });
    }
    output.sort_unstable_by_key(|row| (row.start, row.end));
    output
}

fn episode_groups(events: &[EventSpec]) -> Vec<Vec<usize>> {
    let mut groups = Vec::<Vec<usize>>::new();
    for (index, event) in events.iter().enumerate() {
        if let Some(group) = groups
            .last_mut()
            .filter(|group| events[group[0]].chunk_id == event.chunk_id)
        {
            group.push(index);
        } else {
            groups.push(vec![index]);
        }
    }
    groups.retain(|group| group.len() >= 2);
    groups
}

fn consecutive_event_evidence(events: &[EventSpec]) -> Vec<(usize, usize, Vec<EvidenceId>)> {
    events
        .windows(2)
        .enumerate()
        .filter(|(_, pair)| {
            pair[0]
                .entities
                .iter()
                .any(|id| pair[1].entities.contains(id))
        })
        .map(|(index, pair)| {
            (
                index,
                index + 1,
                merged_evidence([&pair[0].evidence, &pair[1].evidence]),
            )
        })
        .collect()
}

fn causal_pairs(
    text: &str,
    events: &[EventSpec],
) -> Vec<(usize, usize, Vec<EvidenceId>, CausalRelation, f32)> {
    let mut output = Vec::new();
    for (index, pair) in events.windows(2).enumerate() {
        if !pair[0]
            .entities
            .iter()
            .any(|id| pair[1].entities.contains(id))
        {
            continue;
        }
        let lower = source_slice(text, pair[1].start, pair[1].end).to_ascii_lowercase();
        let Some((relation, confidence)) = causal_relation(&lower) else {
            continue;
        };
        output.push((
            index,
            index + 1,
            merged_evidence([&pair[0].evidence, &pair[1].evidence]),
            relation,
            confidence,
        ));
    }
    output
}

type MemorySpec = (
    usize,
    EntityId,
    Vec<EvidenceId>,
    MemoryStateKind,
    &'static str,
    &'static str,
);

fn memory_specs(text: &str, events: &[EventSpec]) -> Vec<MemorySpec> {
    let mut output = Vec::new();
    for (index, event) in events.iter().enumerate() {
        let lower = source_slice(text, event.start, event.end).to_ascii_lowercase();
        let Some((kind, key, value)) = memory_kind(&lower) else {
            continue;
        };
        for entity in &event.entities {
            output.push((index, *entity, event.evidence.clone(), kind, key, value));
        }
    }
    output
}

fn relationship_kind(text: &str) -> Option<(RelationshipKind, f32)> {
    if contains_any(
        text,
        &["father", "daughter", "family", "kiss", "stood beside"],
    ) {
        Some((RelationshipKind::Knows, 0.86))
    } else if contains_any(text, &["gave", "handed", "received", "took it from"]) {
        Some((RelationshipKind::Owns, 0.78))
    } else if contains_any(text, &["entered", "arrived", "stood near", "came in"]) {
        Some((RelationshipKind::LocatedIn, 0.76))
    } else if contains_any(
        text,
        &["warned", "said", "asked", "told", "replied", "called"],
    ) {
        Some((RelationshipKind::CommunicatesWith, 0.84))
    } else if contains_any(text, &["approved", "accepted", "agreed", "supported"]) {
        Some((RelationshipKind::Supports, 0.88))
    } else if contains_any(
        text,
        &["opposed", "attacked", "fought", "refused", "blocked"],
    ) {
        Some((RelationshipKind::Opposes, 0.84))
    } else if contains_any(
        text,
        &["with", "beside", "together", "watched", "saw", "noticed"],
    ) {
        Some((RelationshipKind::ParticipatesIn, 0.68))
    } else {
        None
    }
}

fn event_kind(text: &str) -> Option<EventKind> {
    if contains_any(
        text,
        &["decided", "approved", "accepted", "refused", "agreed"],
    ) {
        Some(EventKind::Decision)
    } else if contains_any(text, &["gave", "handed", "received", "took it from"]) {
        Some(EventKind::Transfer)
    } else if contains_any(
        text,
        &[
            "arrived",
            "entered",
            "met",
            "encountered",
            "warned",
            "said",
            "asked",
            "told",
        ],
    ) {
        Some(EventKind::Encounter)
    } else if contains_any(
        text,
        &[
            "became",
            "changed",
            "remembered",
            "believed",
            "wanted",
            "knew",
        ],
    ) {
        Some(EventKind::StateChange)
    } else if contains_any(
        text,
        &[
            "went", "ran", "moved", "stopped", "left", "opened", "closed", "built", "drove",
            "reached", "looked", "saw",
        ],
    ) {
        Some(EventKind::Action)
    } else {
        None
    }
}

fn causal_relation(text: &str) -> Option<(CausalRelation, f32)> {
    if text.contains("because") {
        Some((CausalRelation::Causes, 0.72))
    } else if contains_any(
        text,
        &["therefore", "as a result", "which meant", "that meant"],
    ) {
        Some((CausalRelation::Causes, 0.70))
    } else if contains_any(text, &["allowed", "enabled", "so that"]) {
        Some((CausalRelation::Enables, 0.68))
    } else if contains_any(text, &["prevented", "stopped", "blocked"]) {
        Some((CausalRelation::Prevents, 0.66))
    } else if contains_any(text, &["wanted", "needed", "motivated"]) {
        Some((CausalRelation::Motivates, 0.64))
    } else {
        None
    }
}

fn memory_kind(text: &str) -> Option<(MemoryStateKind, &'static str, &'static str)> {
    if contains_any(text, &["remembered", "recalled"]) {
        Some((MemoryStateKind::Remembers, "memory", "remembered"))
    } else if contains_any(text, &["believed", "thought", "assumed"]) {
        Some((MemoryStateKind::Believes, "belief", "asserted"))
    } else if contains_any(text, &["wanted", "needed", "hoped"]) {
        Some((MemoryStateKind::Wants, "goal", "wanted"))
    } else if contains_any(text, &["decided", "approved", "refused", "agreed"]) {
        Some((MemoryStateKind::Decides, "decision", "decided"))
    } else if contains_any(
        text,
        &["knew", "recognized", "understood", "noticed", "saw"],
    ) {
        Some((MemoryStateKind::Knows, "knowledge", "observed"))
    } else {
        None
    }
}

fn episode_family<'a>(texts: impl Iterator<Item = &'a str>) -> EpisodeFamily {
    let mut conflict = false;
    let mut transition = false;
    for text in texts {
        let lower = text.to_ascii_lowercase();
        conflict |= contains_any(
            &lower,
            &["attacked", "fought", "refused", "threat", "warned"],
        );
        transition |= contains_any(&lower, &["arrived", "entered", "left", "reached", "moved"]);
    }
    if conflict {
        EpisodeFamily::Conflict
    } else if transition {
        EpisodeFamily::Transition
    } else {
        EpisodeFamily::Sequence
    }
}

fn merged_evidence<'a>(groups: impl IntoIterator<Item = &'a Vec<EvidenceId>>) -> Vec<EvidenceId> {
    let mut output = groups.into_iter().flatten().copied().collect::<Vec<_>>();
    output.sort_unstable();
    output.dedup();
    output
}

fn source_slice(text: &str, start: u32, end: u32) -> &str {
    text.get(start as usize..end as usize).unwrap_or("")
}

fn trim_range(text: &str, start: u32, end: u32) -> (u32, u32) {
    let source = source_slice(text, start, end);
    let leading = source.len() - source.trim_start().len();
    let trailing = source.len() - source.trim_end().len();
    (start + leading as u32, end.saturating_sub(trailing as u32))
}

fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}
