use crate::paint::{build_editor_paint_projection, EditorPaintProjection};
use crate::receipt::append_receipts;
use crate::types::{
    EntityProducerInput, IdentityCandidateInput, IdentityMergeDecision, MentionAuthority,
    UserTaggedEntityInput, UserTaggedMentionInput,
};
use crate::EntityProducerError;
use bytemuck::cast_slice;
use hashbrown::{HashMap, HashSet};
use phoenix_analysis_contract::{AnalysisEntity, AnalysisMention};
use phoenix_graph_generation_v2::{
    CandidateEvidenceBindingRecord, CandidateStatus, CanonicalBindingKind,
    CanonicalEntityBindingRecord, CapabilityRecord, EntityId, EntityRecord, EvidenceRecord,
    EvidenceRole, IdentityCandidateRecord, MentionId, MentionRecord, ModelIdentityRecord,
    PublicationReceiptRecord, StageReceiptRecord, StringRef,
};
use phoenix_scene_compiler::VerifiedStructuralSource;
use phoenix_scene_contract::EntityKind;

const SOURCE_NER: u16 = 1 << 0;
const SOURCE_USER_TAGGED: u16 = 1 << 1;
const MENTION_FLAG_NER: u32 = 1 << 0;
const MENTION_FLAG_USER: u32 = 1 << 1;
const MENTION_FLAG_ACCEPTED_EVIDENCE: u32 = 1 << 2;
const EVIDENCE_FLAG_GRAPH_AUTHORITY: u16 = 1 << 0;

pub(crate) struct BuiltEntityGeneration {
    pub strings: Vec<u8>,
    pub document: phoenix_graph_generation_v2::DocumentRecord,
    pub entities: Vec<EntityRecord>,
    pub mentions: Vec<MentionRecord>,
    pub evidence: Vec<EvidenceRecord>,
    pub identity_candidates: Vec<IdentityCandidateRecord>,
    pub candidate_evidence_bindings: Vec<CandidateEvidenceBindingRecord>,
    pub canonical_entity_bindings: Vec<CanonicalEntityBindingRecord>,
    pub capabilities: Vec<CapabilityRecord>,
    pub model_identities: Vec<ModelIdentityRecord>,
    pub stage_receipts: Vec<StageReceiptRecord>,
    pub publication_receipts: Vec<PublicationReceiptRecord>,
    pub paint: EditorPaintProjection,
    pub authority_hash: [u8; 32],
}

#[derive(Clone)]
struct EntityDraft {
    id: EntityId,
    label: String,
    custom_kind: Option<String>,
    kind: u16,
    source_mask: u16,
    mention_count: u32,
}

pub(crate) fn build_entity_pages(
    input: EntityProducerInput<'_>,
) -> Result<BuiltEntityGeneration, EntityProducerError> {
    input
        .ner
        .validate()
        .map_err(EntityProducerError::InvalidNer)?;
    let source = VerifiedStructuralSource::open(input.structural)?;
    validate_authority(input, &source)?;
    ensure_target_pages_are_empty(input.structural)?;

    let decisions = validate_decisions(
        input.merge_decisions,
        input.user_entities,
        &input.ner.entities,
    )?;
    let mut drafts = build_entities(&input.ner.entities, input.user_entities, &decisions)?;
    let canonical_entity_bindings =
        build_canonical_bindings(&input.ner.entities, input.user_entities, &decisions);
    let canonical_ids = drafts
        .iter()
        .map(|entity| entity.id)
        .collect::<HashSet<_>>();

    let mut authorities = Vec::with_capacity(input.ner.mentions.len() + input.user_mentions.len());
    for mention in &input.ner.mentions {
        authorities.push(bind_ner_mention(input.text, mention, &source, &decisions)?);
    }
    for mention in input.user_mentions {
        authorities.push(bind_user_mention(input.text, mention, &source, &decisions)?);
    }
    let kinds = drafts
        .iter()
        .map(|entity| (entity.id, entity.kind))
        .collect::<HashMap<_, _>>();
    for mention in &mut authorities {
        mention.kind = *kinds
            .get(&mention.entity_id)
            .ok_or(EntityProducerError::ConflictingEntityIdentity)?;
    }
    authorities.sort_unstable_by_key(|mention| mention.mention_id);
    ensure_unique_mentions(&authorities)?;
    count_mentions(&mut drafts, &authorities)?;

    let (mentions, evidence) = pack_mentions(&authorities);
    let (identity_candidates, candidate_evidence_bindings) = build_identity_candidates(
        input.identity_candidates,
        &authorities,
        &canonical_ids,
        &decisions,
    )?;
    let paint = build_editor_paint_projection(&authorities);
    let mut strings = input
        .structural
        .page_bytes(phoenix_graph_generation_v2::PageKind::Strings)
        .to_vec();
    let entities = pack_entities(&mut strings, drafts)?;
    let document = update_document(&source, &entities, &mentions, &evidence)?;

    let mut capabilities = typed_copy(input, phoenix_graph_generation_v2::PageKind::Capabilities)?;
    let mut model_identities = typed_copy(
        input,
        phoenix_graph_generation_v2::PageKind::ModelIdentities,
    )?;
    let mut stage_receipts =
        typed_copy(input, phoenix_graph_generation_v2::PageKind::StageReceipts)?;
    let mut publication_receipts = typed_copy(
        input,
        phoenix_graph_generation_v2::PageKind::PublicationReceipts,
    )?;
    append_receipts(
        input,
        &mut strings,
        &mut capabilities,
        &mut model_identities,
        &mut stage_receipts,
        &mut publication_receipts,
        entities.len(),
        mentions.len(),
        identity_candidates.len(),
        &evidence,
    )?;

    let authority_hash = graph_evidence_hash(
        &entities,
        &mentions,
        &evidence,
        &identity_candidates,
        &candidate_evidence_bindings,
        &canonical_entity_bindings,
    );
    publication_receipts
        .last_mut()
        .ok_or(EntityProducerError::IdentityCollision)?
        .authority_hash = authority_hash;

    Ok(BuiltEntityGeneration {
        strings,
        document,
        entities,
        mentions,
        evidence,
        identity_candidates,
        candidate_evidence_bindings,
        canonical_entity_bindings,
        capabilities,
        model_identities,
        stage_receipts,
        publication_receipts,
        paint,
        authority_hash,
    })
}

fn validate_authority(
    input: EntityProducerInput<'_>,
    source: &VerifiedStructuralSource<'_>,
) -> Result<(), EntityProducerError> {
    let binding = &input.ner.binding;
    let header = input.structural.header();
    let source_hash = blake3::hash(binding.source_document_id.as_bytes());
    if binding.content_hash != header.content_hash
        || binding.native_document_id != header.native_document_id
        || binding.document_revision != header.document_revision
        || binding.target_registry_revision != header.registry_revision
        || *source_hash.as_bytes() != header.source_document_id_hash
        || input.published_generation <= header.published_generation
        || source.document().source_len as usize != input.text.len()
    {
        return Err(EntityProducerError::AuthorityMismatch);
    }
    if blake3::hash(input.text.as_bytes()).as_bytes() != &header.content_hash {
        return Err(EntityProducerError::SourceBindingMismatch);
    }
    Ok(())
}

fn ensure_target_pages_are_empty(
    generation: &phoenix_graph_generation_v2::VerifiedGraphGenerationV2,
) -> Result<(), EntityProducerError> {
    use phoenix_graph_generation_v2::PageKind;
    if [
        PageKind::Entities,
        PageKind::Mentions,
        PageKind::Evidence,
        PageKind::IdentityCandidates,
        PageKind::CandidateEvidenceBindings,
        PageKind::CanonicalEntityBindings,
    ]
    .into_iter()
    .any(|kind| generation.descriptor(kind).count != 0)
    {
        return Err(EntityProducerError::EntityPagesAlreadyPopulated);
    }
    Ok(())
}

fn validate_decisions(
    decisions: &[IdentityMergeDecision],
    user_entities: &[UserTaggedEntityInput<'_>],
    ner_entities: &[AnalysisEntity],
) -> Result<HashMap<EntityId, IdentityMergeDecision>, EntityProducerError> {
    let user_ids = user_entities
        .iter()
        .map(|entity| entity.stable_id)
        .collect::<HashSet<_>>();
    if user_ids.len() != user_entities.len() {
        return Err(EntityProducerError::IdentityCollision);
    }
    let ner_ids = ner_entities
        .iter()
        .map(|entity| EntityId(entity.stable_id))
        .collect::<HashSet<_>>();
    let mut decision_ids = HashSet::with_capacity(decisions.len());
    let mut mappings = HashMap::with_capacity(decisions.len());
    for decision in decisions {
        if decision.decision_id == 0
            || !decision_ids.insert(decision.decision_id)
            || decision.source_user_entity_id.0 == 0
            || decision.canonical_entity_id.0 == 0
            || !user_ids.contains(&decision.source_user_entity_id)
            || ner_ids.contains(&decision.source_user_entity_id)
            || decision.source_user_entity_id == decision.canonical_entity_id
            || (!ner_ids.contains(&decision.canonical_entity_id)
                && !user_ids.contains(&decision.canonical_entity_id))
            || mappings
                .insert(decision.source_user_entity_id, *decision)
                .is_some()
        {
            return Err(EntityProducerError::InvalidMergeDecision);
        }
    }
    if mappings
        .values()
        .any(|decision| mappings.contains_key(&decision.canonical_entity_id))
    {
        return Err(EntityProducerError::InvalidMergeDecision);
    }
    Ok(mappings)
}

fn build_entities(
    ner_entities: &[AnalysisEntity],
    user_entities: &[UserTaggedEntityInput<'_>],
    decisions: &HashMap<EntityId, IdentityMergeDecision>,
) -> Result<Vec<EntityDraft>, EntityProducerError> {
    let mut entities = HashMap::with_capacity(ner_entities.len() + user_entities.len());
    for source in ner_entities {
        let id = EntityId(source.stable_id);
        let draft = EntityDraft {
            id,
            label: source.label.clone(),
            custom_kind: source.custom_kind.clone(),
            kind: source.kind as u16,
            source_mask: SOURCE_NER,
            mention_count: 0,
        };
        if id.0 == 0 || entities.insert(id, draft).is_some() {
            return Err(EntityProducerError::IdentityCollision);
        }
    }
    for source in user_entities
        .iter()
        .filter(|source| !decisions.contains_key(&source.stable_id))
    {
        validate_user_entity(source)?;
        let canonical_id = source.stable_id;
        if let Some(current) = entities.get_mut(&canonical_id) {
            if current.label != source.label
                || current.kind != source.kind as u16
                || current.custom_kind.as_deref() != source.custom_kind
            {
                return Err(EntityProducerError::ConflictingEntityIdentity);
            }
            current.source_mask |= SOURCE_USER_TAGGED;
        } else {
            entities.insert(
                canonical_id,
                EntityDraft {
                    id: canonical_id,
                    label: source.label.to_owned(),
                    custom_kind: source.custom_kind.map(str::to_owned),
                    kind: source.kind as u16,
                    source_mask: SOURCE_USER_TAGGED,
                    mention_count: 0,
                },
            );
        }
    }
    for source in user_entities
        .iter()
        .filter(|source| decisions.contains_key(&source.stable_id))
    {
        validate_user_entity(source)?;
        let canonical_id = canonical_id(decisions, source.stable_id);
        entities
            .get_mut(&canonical_id)
            .ok_or(EntityProducerError::InvalidMergeDecision)?
            .source_mask |= SOURCE_USER_TAGGED;
    }
    let mut entities = entities.into_values().collect::<Vec<_>>();
    entities.sort_unstable_by_key(|entity| entity.id);
    Ok(entities)
}

fn build_canonical_bindings(
    ner_entities: &[AnalysisEntity],
    user_entities: &[UserTaggedEntityInput<'_>],
    decisions: &HashMap<EntityId, IdentityMergeDecision>,
) -> Vec<CanonicalEntityBindingRecord> {
    let mut bindings = HashMap::with_capacity(ner_entities.len() + user_entities.len());
    for entity in ner_entities {
        let id = EntityId(entity.stable_id);
        bindings.insert(
            (id, id, 0_u64, CanonicalBindingKind::Direct as u16),
            SOURCE_NER,
        );
    }
    for entity in user_entities {
        let decision = decisions.get(&entity.stable_id);
        let canonical = decision
            .map(|decision| decision.canonical_entity_id)
            .unwrap_or(entity.stable_id);
        let decision_id = decision.map_or(0, |decision| decision.decision_id);
        let kind = if decision.is_some() {
            CanonicalBindingKind::CoordinatorDecision
        } else {
            CanonicalBindingKind::Direct
        };
        bindings
            .entry((entity.stable_id, canonical, decision_id, kind as u16))
            .and_modify(|mask| *mask |= SOURCE_USER_TAGGED)
            .or_insert(SOURCE_USER_TAGGED);
    }
    let mut records = bindings
        .into_iter()
        .map(
            |((source, canonical, decision_id, kind), source_mask)| CanonicalEntityBindingRecord {
                source_entity_id: source.0,
                canonical_entity_id: canonical.0,
                decision_id,
                source_mask,
                kind,
                flags: 0,
            },
        )
        .collect::<Vec<_>>();
    records.sort_unstable_by_key(|record| {
        (
            record.source_entity_id,
            record.canonical_entity_id,
            record.source_mask,
        )
    });
    records
}

fn validate_user_entity(entity: &UserTaggedEntityInput<'_>) -> Result<(), EntityProducerError> {
    if entity.stable_id.0 == 0
        || entity.label.trim().is_empty()
        || (entity.kind == EntityKind::Custom
            && entity.custom_kind.is_none_or(|kind| kind.trim().is_empty()))
        || (entity.kind != EntityKind::Custom && entity.custom_kind.is_some())
    {
        return Err(EntityProducerError::ConflictingEntityIdentity);
    }
    Ok(())
}

fn bind_ner_mention(
    text: &str,
    mention: &AnalysisMention,
    source: &VerifiedStructuralSource<'_>,
    decisions: &HashMap<EntityId, IdentityMergeDecision>,
) -> Result<MentionAuthority, EntityProducerError> {
    let source_entity = EntityId(mention.entity_id);
    let entity_id = canonical_id(decisions, source_entity);
    let (chunk_id, sentence_index) = resolve_structure(
        text,
        mention.start,
        mention.end,
        Some(mention.sentence_index),
        source,
    )?;
    let mention_id = MentionId(mention.mention_id);
    Ok(MentionAuthority {
        mention_id,
        evidence_id: stable_evidence_id(
            &source.generation().header().content_hash,
            mention_id,
            entity_id,
            mention.start,
            mention.end,
        ),
        entity_id,
        chunk_id,
        start: mention.start,
        end: mention.end,
        sentence_index,
        confidence_bits: mention.confidence.to_bits(),
        flags: MENTION_FLAG_NER
            | if mention.accepted {
                MENTION_FLAG_ACCEPTED_EVIDENCE
            } else {
                0
            },
        source_mask: SOURCE_NER,
        kind: 0,
    })
}

fn bind_user_mention(
    text: &str,
    mention: &UserTaggedMentionInput<'_>,
    source: &VerifiedStructuralSource<'_>,
    decisions: &HashMap<EntityId, IdentityMergeDecision>,
) -> Result<MentionAuthority, EntityProducerError> {
    validate_exact_surface(text, mention.start, mention.end, mention.surface)?;
    let entity_id = canonical_id(decisions, mention.source_entity_id);
    let (chunk_id, sentence_index) =
        resolve_structure(text, mention.start, mention.end, None, source)?;
    let mention_id = stable_user_mention_id(
        &source.generation().header().content_hash,
        mention.source_entity_id,
        mention.start,
        mention.end,
    );
    Ok(MentionAuthority {
        mention_id,
        evidence_id: stable_evidence_id(
            &source.generation().header().content_hash,
            mention_id,
            entity_id,
            mention.start,
            mention.end,
        ),
        entity_id,
        chunk_id,
        start: mention.start,
        end: mention.end,
        sentence_index,
        confidence_bits: 1.0_f32.to_bits(),
        flags: MENTION_FLAG_USER | MENTION_FLAG_ACCEPTED_EVIDENCE,
        source_mask: SOURCE_USER_TAGGED,
        kind: 0,
    })
}

fn resolve_structure(
    text: &str,
    start: u32,
    end: u32,
    declared_sentence: Option<u32>,
    source: &VerifiedStructuralSource<'_>,
) -> Result<(u64, u32), EntityProducerError> {
    validate_range(text, start, end)?;
    let chunk = source
        .chunks()
        .iter()
        .enumerate()
        .filter(|(_, chunk)| chunk.start <= start && end <= chunk.end)
        .min_by_key(|(ordinal, chunk)| (chunk.end - chunk.start, *ordinal))
        .map(|(_, chunk)| chunk)
        .ok_or(EntityProducerError::MissingStructuralBinding)?;
    let mut sentences = source
        .sentences()
        .iter()
        .filter(|sentence| sentence.start <= start && end <= sentence.end);
    let sentence = sentences
        .next()
        .ok_or(EntityProducerError::MissingStructuralBinding)?;
    if sentences.next().is_some()
        || declared_sentence.is_some_and(|value| value != sentence.ordinal)
    {
        return Err(EntityProducerError::MissingStructuralBinding);
    }
    Ok((chunk.id, sentence.ordinal))
}

fn validate_range(text: &str, start: u32, end: u32) -> Result<(), EntityProducerError> {
    let start = start as usize;
    let end = end as usize;
    if start >= end
        || end > text.len()
        || !text.is_char_boundary(start)
        || !text.is_char_boundary(end)
    {
        return Err(EntityProducerError::InvalidMentionRange);
    }
    Ok(())
}

fn validate_exact_surface(
    text: &str,
    start: u32,
    end: u32,
    surface: &str,
) -> Result<(), EntityProducerError> {
    validate_range(text, start, end)?;
    if &text[start as usize..end as usize] != surface {
        return Err(EntityProducerError::InvalidMentionRange);
    }
    Ok(())
}

fn ensure_unique_mentions(authority: &[MentionAuthority]) -> Result<(), EntityProducerError> {
    if authority.iter().any(|mention| {
        mention.mention_id.0 == 0 || mention.evidence_id.0 == 0 || mention.entity_id.0 == 0
    }) || authority
        .windows(2)
        .any(|pair| pair[0].mention_id == pair[1].mention_id)
    {
        return Err(EntityProducerError::IdentityCollision);
    }
    let evidence = authority
        .iter()
        .map(|mention| mention.evidence_id)
        .collect::<HashSet<_>>();
    if evidence.len() != authority.len() {
        return Err(EntityProducerError::IdentityCollision);
    }
    Ok(())
}

fn count_mentions(
    entities: &mut [EntityDraft],
    authority: &[MentionAuthority],
) -> Result<(), EntityProducerError> {
    let index = entities
        .iter()
        .enumerate()
        .map(|(index, entity)| (entity.id, index))
        .collect::<HashMap<_, _>>();
    for mention in authority {
        let slot = *index
            .get(&mention.entity_id)
            .ok_or(EntityProducerError::ConflictingEntityIdentity)?;
        entities[slot].mention_count = entities[slot]
            .mention_count
            .checked_add(1)
            .ok_or(EntityProducerError::RecordCountOverflow)?;
    }
    Ok(())
}

fn pack_mentions(authority: &[MentionAuthority]) -> (Vec<MentionRecord>, Vec<EvidenceRecord>) {
    let mut mentions = Vec::with_capacity(authority.len());
    let mut evidence = Vec::with_capacity(authority.len());
    for mention in authority {
        mentions.push(MentionRecord {
            id: mention.mention_id.0,
            entity_id: mention.entity_id.0,
            evidence_id: mention.evidence_id.0,
            chunk_id: mention.chunk_id,
            start: mention.start,
            end: mention.end,
            sentence_index: mention.sentence_index,
            confidence_bits: mention.confidence_bits,
            flags: mention.flags,
            reserved: 0,
        });
        evidence.push(EvidenceRecord {
            id: mention.evidence_id.0,
            entity_id: mention.entity_id.0,
            mention_id: mention.mention_id.0,
            chunk_id: mention.chunk_id,
            start: mention.start,
            end: mention.end,
            role: EvidenceRole::Subject as u16,
            flags: EVIDENCE_FLAG_GRAPH_AUTHORITY,
            reserved: 0,
        });
    }
    (mentions, evidence)
}

fn build_identity_candidates(
    inputs: &[IdentityCandidateInput],
    authority: &[MentionAuthority],
    canonical_ids: &HashSet<EntityId>,
    decisions: &HashMap<EntityId, IdentityMergeDecision>,
) -> Result<
    (
        Vec<IdentityCandidateRecord>,
        Vec<CandidateEvidenceBindingRecord>,
    ),
    EntityProducerError,
> {
    let mentions = authority
        .iter()
        .map(|mention| (mention.mention_id, mention))
        .collect::<HashMap<_, _>>();
    let mut sorted = inputs.to_vec();
    sorted.sort_unstable_by_key(|candidate| candidate.candidate_id);
    if sorted
        .windows(2)
        .any(|pair| pair[0].candidate_id == pair[1].candidate_id)
    {
        return Err(EntityProducerError::IdentityCollision);
    }
    let mut records = Vec::with_capacity(sorted.len());
    let mut bindings = Vec::with_capacity(sorted.len().saturating_mul(2));
    for input in sorted {
        let left_entity = canonical_id(decisions, input.left_entity_id);
        let right_entity = canonical_id(decisions, input.right_entity_id);
        let left_mention = mentions
            .get(&input.left_mention_id)
            .ok_or(EntityProducerError::InvalidIdentityCandidate)?;
        let right_mention = mentions
            .get(&input.right_mention_id)
            .ok_or(EntityProducerError::InvalidIdentityCandidate)?;
        if input.candidate_id.is_zero()
            || left_entity == right_entity
            || !canonical_ids.contains(&left_entity)
            || !canonical_ids.contains(&right_entity)
            || left_mention.entity_id != left_entity
            || right_mention.entity_id != right_entity
            || !input.confidence.is_finite()
            || !(0.0..=1.0).contains(&input.confidence)
        {
            return Err(EntityProducerError::InvalidIdentityCandidate);
        }
        let evidence_start =
            u32::try_from(bindings.len()).map_err(|_| EntityProducerError::RecordCountOverflow)?;
        bindings.push(CandidateEvidenceBindingRecord {
            candidate_id: input.candidate_id,
            evidence_id: left_mention.evidence_id.0,
            ordinal: 0,
            role: EvidenceRole::Source as u16,
            flags: 0,
        });
        bindings.push(CandidateEvidenceBindingRecord {
            candidate_id: input.candidate_id,
            evidence_id: right_mention.evidence_id.0,
            ordinal: 1,
            role: EvidenceRole::Target as u16,
            flags: 0,
        });
        records.push(IdentityCandidateRecord {
            candidate_id: input.candidate_id,
            left_entity_id: left_entity.0,
            right_entity_id: right_entity.0,
            evidence_start,
            evidence_count: 2,
            confidence_bits: input.confidence.to_bits(),
            flags: 0,
            kind: input.kind as u16,
            status: CandidateStatus::Proposed as u16,
            reserved: 0,
        });
    }
    Ok((records, bindings))
}

fn pack_entities(
    strings: &mut Vec<u8>,
    drafts: Vec<EntityDraft>,
) -> Result<Vec<EntityRecord>, EntityProducerError> {
    let mut entities = Vec::with_capacity(drafts.len());
    for draft in drafts {
        entities.push(EntityRecord {
            id: draft.id.0,
            label: push_string(strings, &draft.label)?,
            custom_kind: push_string(strings, draft.custom_kind.as_deref().unwrap_or(""))?,
            mention_count: draft.mention_count,
            kind: draft.kind,
            source_mask: draft.source_mask,
            flags: 0,
            reserved: 0,
        });
    }
    Ok(entities)
}

fn update_document(
    source: &VerifiedStructuralSource<'_>,
    entities: &[EntityRecord],
    mentions: &[MentionRecord],
    evidence: &[EvidenceRecord],
) -> Result<phoenix_graph_generation_v2::DocumentRecord, EntityProducerError> {
    let mut document = *source.document();
    document.entity_count =
        u32::try_from(entities.len()).map_err(|_| EntityProducerError::RecordCountOverflow)?;
    document.mention_count =
        u32::try_from(mentions.len()).map_err(|_| EntityProducerError::RecordCountOverflow)?;
    document.evidence_count =
        u32::try_from(evidence.len()).map_err(|_| EntityProducerError::RecordCountOverflow)?;
    Ok(document)
}

fn typed_copy<T: bytemuck::Pod + Copy>(
    input: EntityProducerInput<'_>,
    page: phoenix_graph_generation_v2::PageKind,
) -> Result<Vec<T>, EntityProducerError> {
    Ok(input.structural.typed_page::<T>(page)?.to_vec())
}

fn push_string(strings: &mut Vec<u8>, value: &str) -> Result<StringRef, EntityProducerError> {
    let offset = strings.len() as u64;
    let length =
        u32::try_from(value.len()).map_err(|_| EntityProducerError::RecordCountOverflow)?;
    strings.extend_from_slice(value.as_bytes());
    Ok(StringRef {
        offset,
        length,
        reserved: 0,
    })
}

fn stable_user_mention_id(
    content_hash: &[u8; 32],
    source_entity_id: EntityId,
    start: u32,
    end: u32,
) -> MentionId {
    MentionId(stable_id(
        b"user-mention",
        content_hash,
        &[source_entity_id.0, u64::from(start), u64::from(end)],
    ))
}

fn canonical_id(
    decisions: &HashMap<EntityId, IdentityMergeDecision>,
    source: EntityId,
) -> EntityId {
    decisions
        .get(&source)
        .map(|decision| decision.canonical_entity_id)
        .unwrap_or(source)
}

fn stable_evidence_id(
    content_hash: &[u8; 32],
    mention_id: MentionId,
    entity_id: EntityId,
    start: u32,
    end: u32,
) -> phoenix_graph_generation_v2::EvidenceId {
    phoenix_graph_generation_v2::EvidenceId(stable_id(
        b"evidence",
        content_hash,
        &[mention_id.0, entity_id.0, u64::from(start), u64::from(end)],
    ))
}

pub(crate) fn stable_id(domain: &[u8], content_hash: &[u8; 32], values: &[u64]) -> u64 {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"phoenix.entity-producer/v1/id\0");
    hasher.update(domain);
    hasher.update(content_hash);
    for value in values {
        hasher.update(&value.to_le_bytes());
    }
    let mut bytes = [0_u8; 8];
    bytes.copy_from_slice(&hasher.finalize().as_bytes()[..8]);
    let id = u64::from_le_bytes(bytes);
    if id == 0 {
        1
    } else {
        id
    }
}

fn graph_evidence_hash(
    entities: &[EntityRecord],
    mentions: &[MentionRecord],
    evidence: &[EvidenceRecord],
    identity: &[IdentityCandidateRecord],
    bindings: &[CandidateEvidenceBindingRecord],
    canonical_bindings: &[CanonicalEntityBindingRecord],
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"phoenix.entity-producer/v1/graph-evidence\0");
    hasher.update(cast_slice(entities));
    hasher.update(cast_slice(mentions));
    hasher.update(cast_slice(evidence));
    hasher.update(cast_slice(identity));
    hasher.update(cast_slice(bindings));
    hasher.update(cast_slice(canonical_bindings));
    *hasher.finalize().as_bytes()
}
