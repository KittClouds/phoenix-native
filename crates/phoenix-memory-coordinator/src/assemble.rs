use crate::{
    authoritative_product, fingerprint::state_hash, product::candidate_product, CommonProducts,
    CoordinatorError, CoordinatorState, DualFaceProducer, EntityDraft, ProducerProductV3,
    RegistrationSupport, SemanticCandidateFamilyV3, StoredDocument, StoredTurn,
    VocabularyPackDraft, NO_MODEL_IDENTITY,
};
use hashbrown::{HashMap, HashSet};
use phoenix_analysis_contract::StructuralSpanKind;
use phoenix_memory_contract::{
    deterministic_id, CandidateEndpointBindingRecordV3, CandidateEvidenceBindingRecord,
    CandidateId, CanonicalEntityBindingRecord, ChapterRecord, ContentUnitKind, ContentUnitRecord,
    ConversationInput, DocumentChunkInput, DocumentInput, EntityRecord, EvidenceRecordV3,
    EvidenceRole, MentionRecordV3, MixedSourceBuilder, ModelIdentityRecord, ParagraphRecord,
    PreparedMixedSource, ProducerCapabilityRecordV3, ProducerStateV3, PublicationReceiptRecord,
    PublicationStatus, SemanticCandidateRecordV3, SentenceRecord, SourceKind, SpanRecord,
    StringRef, TemporalEnvelopeBindingRecordV1, TemporalEnvelopeRecordV1, TurnInput,
    VocabularyPackRecordV3,
};

pub(crate) fn prepare_generation<P: DualFaceProducer>(
    state: &CoordinatorState<P>,
    published_generation: u64,
) -> Result<([u8; 32], PreparedMixedSource), CoordinatorError> {
    let state_hash = state_hash(state);
    let mut builder = MixedSourceBuilder::from_namespace_hash(state.namespace_hash).generations(
        state.config.registry_revision,
        published_generation,
        published_generation,
    );

    let mut documents = state.documents.values().collect::<Vec<_>>();
    documents.sort_unstable_by_key(|stored| stored.request.lease.entry_id.0);
    for stored in &documents {
        builder = builder.add_document(document_input(stored, published_generation));
    }
    let mut conversations = state.conversations.values().collect::<Vec<_>>();
    conversations.sort_unstable_by(|left, right| left.external_id.cmp(&right.external_id));
    for conversation in &conversations {
        let turns = conversation
            .turns
            .iter()
            .map(|stored| TurnInput {
                external_id: stored.turn.external_id.to_vec(),
                ordinal: stored.turn.ordinal,
                role: stored.turn.role,
                event_time_millis: stored.turn.event_time_millis,
                reply_to_ordinal: stored.turn.reply_to_ordinal,
                actor_entity_id: stored.turn.actor_entity_id,
                model_identity_index: stored.turn.model_identity_index,
                content: stored.turn.content.to_string(),
                flags: 0,
            })
            .collect();
        let ended_at_millis = conversation
            .turns
            .last()
            .map_or(conversation.started_at_millis, |stored| {
                stored.turn.event_time_millis
            });
        builder = builder.add_conversation(ConversationInput {
            external_id: conversation.external_id.to_vec(),
            started_at_millis: conversation.started_at_millis,
            ended_at_millis,
            turns,
        });
    }
    let mut prepared = builder.prepare()?;
    append_models(state, &mut prepared)?;
    append_document_structure(state, &documents, &mut prepared)?;
    append_common_products(state, &documents, &conversations, &mut prepared)?;
    append_capabilities(state, &mut prepared)?;
    prepared
        .pages
        .publication_receipts
        .push(PublicationReceiptRecord {
            authority_hash: state_hash,
            previous_generation_hash: state
                .current
                .as_ref()
                .map_or([0; 32], |receipt| receipt.generation_hash),
            generation_id: published_generation,
            previous_generation_id: state.published_generation,
            document_revision: documents
                .iter()
                .map(|stored| stored.request.revision.0)
                .max()
                .unwrap_or(0),
            registry_revision: state.config.registry_revision,
            published_at_unix_millis: 0,
            status: PublicationStatus::Published as u16,
            flags_u16: 0,
            flags: 0,
        });
    Ok((state_hash, prepared))
}

fn document_input(stored: &StoredDocument, generation: u64) -> DocumentInput {
    let chunks = stored
        .production
        .structural
        .chunks
        .iter()
        .map(|chunk| DocumentChunkInput {
            start: chunk.start,
            end: chunk.end,
            sentence_start: chunk.sentence_start,
            sentence_end: chunk.sentence_end,
            paragraph_start: chunk.paragraph_start,
            paragraph_end: chunk.paragraph_end,
            chapter_index: chunk.chapter_index,
            token_count: chunk.token_count,
            flags: 0,
        })
        .collect();
    DocumentInput::current(
        stored.request.lease.entry_id.0.to_le_bytes(),
        stored.request.revision.0,
        format!("workspace-entry/{:016x}", stored.request.lease.entry_id.0),
        stored.request.lease.content.to_string(),
        chunks,
        generation,
    )
}

fn append_document_structure<P: DualFaceProducer>(
    state: &CoordinatorState<P>,
    documents: &[&StoredDocument],
    prepared: &mut PreparedMixedSource,
) -> Result<(), CoordinatorError> {
    for stored in documents {
        let source_id = source_id(
            state.namespace_hash,
            SourceKind::WorkspaceDocument,
            &stored.request.lease.entry_id.0.to_le_bytes(),
        );
        let revision = prepared
            .pages
            .document_revisions
            .iter()
            .find(|record| record.source_id == source_id)
            .ok_or(CoordinatorError::ProducerAuthority(
                "prepared document revision does not resolve",
            ))?;
        let document_id = revision.document_id;
        let document_unit_id = prepared
            .pages
            .content_units
            .iter()
            .find(|unit| {
                unit.source_id == source_id && unit.kind == ContentUnitKind::Document as u16
            })
            .map(|unit| unit.id)
            .ok_or(CoordinatorError::ProducerAuthority(
                "prepared document content unit does not resolve",
            ))?;
        append_structural_rows(source_id, document_id, document_unit_id, stored, prepared)?;
    }
    Ok(())
}

fn append_structural_rows(
    source_id: u64,
    document_id: u64,
    document_unit_id: u64,
    stored: &StoredDocument,
    prepared: &mut PreparedMixedSource,
) -> Result<(), CoordinatorError> {
    let structural = &stored.production.structural;
    let chapter_spans = structural
        .spans
        .iter()
        .filter(|span| span.kind == StructuralSpanKind::Chapter)
        .collect::<Vec<_>>();
    let paragraph_spans = structural
        .spans
        .iter()
        .filter(|span| span.kind == StructuralSpanKind::Paragraph)
        .collect::<Vec<_>>();
    let chapter_ids = chapter_spans
        .iter()
        .enumerate()
        .map(|(ordinal, span)| structural_id(b"chapter", source_id, ordinal, span.content_hash))
        .collect::<Vec<_>>();
    let paragraph_ids = paragraph_spans
        .iter()
        .enumerate()
        .map(|(ordinal, span)| structural_id(b"paragraph", source_id, ordinal, span.content_hash))
        .collect::<Vec<_>>();

    for (ordinal, span) in chapter_spans.iter().enumerate() {
        let id = chapter_ids[ordinal];
        let title = append_string(&mut prepared.pages.strings, span.label.as_bytes())?;
        prepared.pages.chapters.push(ChapterRecord {
            id,
            document_id,
            title,
            start: span.start,
            end: span.end,
            paragraph_start: span.child_start,
            paragraph_end: span.child_end,
            ordinal: ordinal as u32,
            flags: 0,
            reserved: [0; 2],
        });
        push_content_unit(
            &mut prepared.pages.content_units,
            id,
            source_id,
            document_id,
            document_unit_id,
            span.start,
            span.end,
            ordinal as u32,
            span.token_count,
            ContentUnitKind::Chapter,
            span.content_hash,
        );
    }
    for (ordinal, span) in paragraph_spans.iter().enumerate() {
        let id = paragraph_ids[ordinal];
        let chapter_id = *chapter_ids.get(span.parent_index as usize).ok_or(
            CoordinatorError::ProducerAuthority("paragraph chapter does not resolve"),
        )?;
        prepared.pages.paragraphs.push(ParagraphRecord {
            id,
            document_id,
            chapter_id,
            start: span.start,
            end: span.end,
            sentence_start: span.child_start,
            sentence_end: span.child_end,
            ordinal: ordinal as u32,
            flags: 0,
            reserved: [0; 2],
        });
        push_content_unit(
            &mut prepared.pages.content_units,
            id,
            source_id,
            document_id,
            chapter_id,
            span.start,
            span.end,
            ordinal as u32,
            span.token_count,
            ContentUnitKind::Paragraph,
            span.content_hash,
        );
    }
    for (ordinal, sentence) in structural.sentences.iter().enumerate() {
        let paragraph_id = *paragraph_ids.get(sentence.paragraph_index as usize).ok_or(
            CoordinatorError::ProducerAuthority("sentence paragraph does not resolve"),
        )?;
        let id = structural_id(b"sentence", source_id, ordinal, sentence.content_hash);
        prepared.pages.sentences.push(SentenceRecord {
            id,
            document_id,
            paragraph_id,
            content_hash: sentence.content_hash,
            start: sentence.start,
            end: sentence.end,
            ordinal: ordinal as u32,
            token_count: sentence.token_count,
            quality: sentence.quality as u16,
            dialogue_hint: sentence.dialogue_hint as u16,
            flags: 0,
            reserved_u16: 0,
            reserved: 0,
        });
        push_content_unit(
            &mut prepared.pages.content_units,
            id,
            source_id,
            document_id,
            paragraph_id,
            sentence.start,
            sentence.end,
            ordinal as u32,
            sentence.token_count,
            ContentUnitKind::Sentence,
            sentence.content_hash,
        );
    }
    for (ordinal, span) in structural.spans.iter().enumerate() {
        let parent_id = match span.kind {
            StructuralSpanKind::Chapter => 0,
            StructuralSpanKind::Paragraph => *chapter_ids.get(span.parent_index as usize).ok_or(
                CoordinatorError::ProducerAuthority("span parent does not resolve"),
            )?,
        };
        let label = append_string(&mut prepared.pages.strings, span.label.as_bytes())?;
        prepared.pages.spans.push(SpanRecord {
            id: structural_id(b"span", source_id, ordinal, span.content_hash),
            document_id,
            parent_id,
            content_hash: span.content_hash,
            label,
            start: span.start,
            end: span.end,
            child_start: span.child_start,
            child_end: span.child_end,
            token_count: span.token_count,
            flags: 0,
            kind: span.kind as u16,
            dialogue_hint: span.dialogue_hint as u16,
            reserved: 0,
        });
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn push_content_unit(
    units: &mut Vec<ContentUnitRecord>,
    id: u64,
    source_id: u64,
    owner_id: u64,
    parent_id: u64,
    start: u32,
    end: u32,
    ordinal: u32,
    token_count: u32,
    kind: ContentUnitKind,
    content_hash: u64,
) {
    let mut hash = [0_u8; 32];
    hash[..8].copy_from_slice(&content_hash.to_le_bytes());
    units.push(ContentUnitRecord {
        id,
        source_id,
        owner_id,
        parent_id,
        content_hash: hash,
        start,
        end,
        ordinal,
        token_count,
        kind: kind as u16,
        flags: 1,
        reserved: 0,
    });
}

fn append_models<P: DualFaceProducer>(
    state: &CoordinatorState<P>,
    prepared: &mut PreparedMixedSource,
) -> Result<(), CoordinatorError> {
    for model in state.config.model_identities.iter() {
        prepared.pages.model_identities.push(ModelIdentityRecord {
            name: append_string(&mut prepared.pages.strings, model.name.as_bytes())?,
            runtime: append_string(&mut prepared.pages.strings, model.runtime.as_bytes())?,
            artifact_uri: append_string(
                &mut prepared.pages.strings,
                model.artifact_uri.as_bytes(),
            )?,
            artifact_hash: model.artifact_hash,
            config_hash: model.config_hash,
            flags: model.semantic_role.flags(),
            reserved: 0,
        });
    }
    Ok(())
}

fn append_common_products<P: DualFaceProducer>(
    state: &CoordinatorState<P>,
    documents: &[&StoredDocument],
    conversations: &[&crate::StoredConversation],
    prepared: &mut PreparedMixedSource,
) -> Result<(), CoordinatorError> {
    let mut entities = HashMap::<u64, EntityDraft>::new();
    let mut entity_sources = HashSet::new();
    let mut evidence_ids = HashSet::new();
    let mut candidate_ids = HashSet::new();
    let mut temporal_envelope_ids = HashSet::new();
    let mut vocabulary_packs = HashMap::<u64, VocabularyPackDraft>::new();

    for stored in documents {
        let external_id = stored.request.lease.entry_id.0.to_le_bytes();
        let source_id = source_id(
            state.namespace_hash,
            SourceKind::WorkspaceDocument,
            &external_id,
        );
        let _document = prepared
            .pages
            .document_revisions
            .iter()
            .find(|record| record.source_id == source_id)
            .ok_or(CoordinatorError::ProducerAuthority(
                "document products have no source",
            ))?;
        let document_unit = prepared
            .pages
            .content_units
            .iter()
            .find(|unit| {
                unit.source_id == source_id && unit.kind == ContentUnitKind::Document as u16
            })
            .map(|unit| unit.id)
            .ok_or(CoordinatorError::ProducerAuthority(
                "document products have no content unit",
            ))?;
        let chunk_units = prepared
            .pages
            .content_units
            .iter()
            .filter(|unit| {
                unit.source_id == source_id && unit.kind == ContentUnitKind::DynamicChunk as u16
            })
            .map(|unit| (unit.start, unit.end, unit.id))
            .collect::<Vec<_>>();
        append_source_products(
            source_id,
            document_unit,
            &chunk_units,
            &stored.production.common,
            &mut entities,
            &mut entity_sources,
            &mut evidence_ids,
            &mut candidate_ids,
            &mut temporal_envelope_ids,
            &mut vocabulary_packs,
            prepared,
            state.published_generation.saturating_add(1),
        )?;
    }

    for conversation in conversations {
        let source_id = source_id(
            state.namespace_hash,
            SourceKind::Conversation,
            &conversation.external_id,
        );
        let conversation_id = deterministic_id(
            b"conversation",
            &[&state.namespace_hash, &source_id.to_le_bytes()],
        );
        for stored in &conversation.turns {
            let turn_id = turn_id(state.namespace_hash, source_id, conversation_id, stored);
            let unit_id = prepared
                .pages
                .content_units
                .iter()
                .find(|unit| {
                    unit.source_id == source_id
                        && unit.owner_id == turn_id
                        && unit.kind == ContentUnitKind::Turn as u16
                })
                .map(|unit| unit.id)
                .ok_or(CoordinatorError::ProducerAuthority(
                    "turn products have no content unit",
                ))?;
            append_source_products(
                source_id,
                unit_id,
                &[],
                &stored.production.common,
                &mut entities,
                &mut entity_sources,
                &mut evidence_ids,
                &mut candidate_ids,
                &mut temporal_envelope_ids,
                &mut vocabulary_packs,
                prepared,
                state.published_generation.saturating_add(1),
            )?;
        }
    }

    let mut entities = entities.into_values().collect::<Vec<_>>();
    entities.sort_unstable_by_key(|entity| entity.stable_id);
    for entity in entities {
        prepared.pages.entities.push(EntityRecord {
            id: entity.stable_id,
            label: append_string(&mut prepared.pages.strings, entity.label.as_bytes())?,
            custom_kind: match entity.custom_kind {
                Some(value) => append_string(&mut prepared.pages.strings, value.as_bytes())?,
                None => StringRef::default(),
            },
            mention_count: entity.mention_count,
            kind: entity.kind,
            source_mask: entity.source_mask,
            flags: 0,
            reserved: 0,
        });
    }
    let mut packs = vocabulary_packs.into_values().collect::<Vec<_>>();
    packs.sort_unstable_by_key(|pack| pack.id);
    for pack in packs {
        prepared
            .pages
            .vocabulary_packs
            .push(VocabularyPackRecordV3 {
                id: pack.id,
                name: append_string(&mut prepared.pages.strings, pack.name.as_bytes())?,
                version: append_string(&mut prepared.pages.strings, pack.version.as_bytes())?,
                schema_hash: pack.schema_hash,
                producer_identity_hash: pack.producer_identity_hash,
                kind: pack.kind as u16,
                flags: pack.flags,
                reserved: 0,
            });
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn append_source_products(
    source_id: u64,
    default_unit_id: u64,
    ranged_units: &[(u32, u32, u64)],
    products: &CommonProducts,
    entities: &mut HashMap<u64, EntityDraft>,
    entity_sources: &mut HashSet<(u64, u64)>,
    evidence_ids: &mut HashSet<u64>,
    candidate_ids: &mut HashSet<[u8; 32]>,
    temporal_envelope_ids: &mut HashSet<[u8; 32]>,
    vocabulary_packs: &mut HashMap<u64, VocabularyPackDraft>,
    prepared: &mut PreparedMixedSource,
    system_generation: u64,
) -> Result<(), CoordinatorError> {
    for pack in &products.vocabulary_packs {
        match vocabulary_packs.get(&pack.id) {
            Some(existing) if existing != pack => return Err(CoordinatorError::InvalidCandidate),
            Some(_) => {}
            None => {
                vocabulary_packs.insert(pack.id, pack.clone());
            }
        }
    }
    for entity in &products.entities {
        if !entity_sources.insert((source_id, entity.stable_id)) {
            return Err(CoordinatorError::ProducerAuthority(
                "source repeats a canonical entity",
            ));
        }
        match entities.get_mut(&entity.stable_id) {
            Some(current)
                if current.label == entity.label
                    && current.kind == entity.kind
                    && current.custom_kind == entity.custom_kind =>
            {
                current.mention_count = current
                    .mention_count
                    .checked_add(entity.mention_count)
                    .ok_or(CoordinatorError::Oversized)?;
                current.source_mask |= entity.source_mask;
            }
            Some(_) => {
                return Err(CoordinatorError::ProducerAuthority(
                    "stable entity identity has conflicting metadata",
                ))
            }
            None => {
                entities.insert(entity.stable_id, entity.clone());
            }
        }
    }
    for binding in &products.canonical_bindings {
        prepared
            .pages
            .canonical_entity_bindings
            .push(CanonicalEntityBindingRecord {
                source_entity_id: binding.source_entity_id,
                canonical_entity_id: binding.canonical_entity_id,
                decision_id: binding.decision_id,
                source_mask: binding.source_mask,
                kind: binding.kind as u16,
                flags: 0,
            });
    }
    for mention in &products.mentions {
        if !evidence_ids.insert(mention.evidence_id) {
            return Err(CoordinatorError::ProducerAuthority(
                "evidence identity is duplicated",
            ));
        }
        let content_unit_id = ranged_units
            .iter()
            .find(|(start, end, _)| *start <= mention.start && *end >= mention.end)
            .map_or(default_unit_id, |(_, _, id)| *id);
        prepared.pages.mentions.push(MentionRecordV3 {
            id: mention.stable_id,
            source_id,
            entity_id: mention.entity_id,
            evidence_id: mention.evidence_id,
            content_unit_id,
            start: mention.start,
            end: mention.end,
            confidence_bits: mention.confidence.to_bits(),
            flags: mention.flags,
        });
        prepared.pages.evidence.push(EvidenceRecordV3 {
            id: mention.evidence_id,
            source_id,
            entity_id: mention.entity_id,
            mention_id: mention.stable_id,
            content_unit_id,
            start: mention.start,
            end: mention.end,
            role: EvidenceRole::Source as u16,
            flags: 0,
            reserved: 0,
        });
    }
    for candidate in &products.candidates {
        if !candidate_ids.insert(candidate.candidate_id)
            || candidate
                .evidence_ids
                .iter()
                .any(|evidence| !evidence_ids.contains(evidence))
        {
            return Err(CoordinatorError::InvalidCandidate);
        }
        let endpoint_start = u32::try_from(prepared.pages.candidate_endpoint_bindings.len())
            .map_err(|_| CoordinatorError::Oversized)?;
        for (ordinal, endpoint) in candidate.endpoints.iter().enumerate() {
            prepared
                .pages
                .candidate_endpoint_bindings
                .push(CandidateEndpointBindingRecordV3 {
                    candidate_id: candidate.candidate_id,
                    endpoint_id: endpoint.endpoint_id,
                    ordinal: u32::try_from(ordinal).map_err(|_| CoordinatorError::Oversized)?,
                    role: endpoint.role as u16,
                    flags: endpoint.flags,
                });
        }
        let evidence_start = u32::try_from(prepared.pages.candidate_evidence_bindings.len())
            .map_err(|_| CoordinatorError::Oversized)?;
        for (ordinal, evidence_id) in candidate.evidence_ids.iter().enumerate() {
            prepared
                .pages
                .candidate_evidence_bindings
                .push(CandidateEvidenceBindingRecord {
                    candidate_id: CandidateId(candidate.candidate_id),
                    evidence_id: *evidence_id,
                    ordinal: u32::try_from(ordinal).map_err(|_| CoordinatorError::Oversized)?,
                    role: EvidenceRole::Premise as u16,
                    flags: 0,
                });
        }
        prepared
            .pages
            .semantic_candidates
            .push(SemanticCandidateRecordV3 {
                candidate_id: candidate.candidate_id,
                source_id,
                vocabulary_pack_id: candidate.vocabulary_pack_id,
                relation_kind: append_string(
                    &mut prepared.pages.strings,
                    candidate.relation_kind.as_bytes(),
                )?,
                value: append_string(&mut prepared.pages.strings, candidate.value.as_bytes())?,
                endpoint_start,
                endpoint_count: candidate.endpoints.len() as u32,
                evidence_start,
                evidence_count: candidate.evidence_ids.len() as u32,
                valid_time_from_millis: candidate.valid_time_from_millis,
                valid_time_to_millis: candidate.valid_time_to_millis,
                system_generation_from: system_generation,
                system_generation_to: u64::MAX,
                confidence_bits: candidate.confidence.to_bits(),
                model_identity_index: candidate.model_identity_index.unwrap_or(NO_MODEL_IDENTITY),
                producer_identity_hash: candidate.producer_identity_hash,
                family: candidate.family as u16,
                status: candidate.status as u16,
                flags: candidate.flags,
                reserved: [0; 2],
            });
    }
    for envelope in &products.temporal_envelopes {
        if !temporal_envelope_ids.insert(envelope.id) {
            return Err(CoordinatorError::InvalidCandidate);
        }
        let binding_start = u32::try_from(prepared.pages.temporal_envelope_bindings.len())
            .map_err(|_| CoordinatorError::Oversized)?;
        for (ordinal, binding) in envelope.bindings.iter().enumerate() {
            prepared
                .pages
                .temporal_envelope_bindings
                .push(TemporalEnvelopeBindingRecordV1 {
                    envelope_id: envelope.id,
                    subject_id: binding.subject_id,
                    evidence_id: binding.evidence_id,
                    ordinal: u32::try_from(ordinal).map_err(|_| CoordinatorError::Oversized)?,
                    subject_kind: binding.subject_kind as u16,
                    role: binding.role as u16,
                    flags: binding.flags,
                    reserved: 0,
                });
        }
        prepared
            .pages
            .temporal_envelopes
            .push(TemporalEnvelopeRecordV1 {
                id: envelope.id,
                source_time_millis: envelope.source_time_millis,
                asserted_at_millis: envelope.asserted_at_millis,
                occurred_from_millis: envelope.occurred_from_millis,
                occurred_to_millis: envelope.occurred_to_millis,
                observed_at_millis: envelope.observed_at_millis,
                valid_time_from_millis: envelope.valid_time_from_millis,
                valid_time_to_millis: envelope.valid_time_to_millis,
                system_generation_from: system_generation,
                system_generation_to: u64::MAX,
                original_text: if envelope.original_text.is_empty() {
                    StringRef::default()
                } else {
                    append_string(
                        &mut prepared.pages.strings,
                        envelope.original_text.as_bytes(),
                    )?
                },
                binding_start,
                binding_count: envelope.bindings.len() as u32,
                timezone_offset_minutes: envelope.timezone_offset_minutes,
                confidence_bits: envelope.confidence.to_bits(),
                precision: envelope.precision as u16,
                reserved_u16: 0,
                flags: envelope.flags,
                reserved: [0; 2],
            });
    }
    Ok(())
}

fn append_capabilities<P: DualFaceProducer>(
    state: &CoordinatorState<P>,
    prepared: &mut PreparedMixedSource,
) -> Result<(), CoordinatorError> {
    let counts = product_counts(&prepared.pages);
    for registration in state.config.registrations.iter() {
        let state = match registration.support {
            RegistrationSupport::Supported => ProducerStateV3::Produced,
            RegistrationSupport::Unsupported => ProducerStateV3::Unsupported,
        };
        let output_count = if state == ProducerStateV3::Unsupported {
            0
        } else {
            counts[(registration.product as usize) - 1]
        };
        prepared
            .pages
            .producer_capabilities_v3
            .push(ProducerCapabilityRecordV3 {
                producer: append_string(
                    &mut prepared.pages.strings,
                    registration.producer.as_bytes(),
                )?,
                producer_binary_hash: registration.producer_binary_hash,
                config_hash: registration.config_hash,
                output_count,
                reused_generation: 0,
                model_identity_index: registration
                    .model_identity_index
                    .unwrap_or(NO_MODEL_IDENTITY),
                product: registration.product as u16,
                state: state as u16,
                flags_u16: u16::from(authoritative_product(registration.product)),
                padding_u16: 0,
                flags: 0,
                reserved: [0; 2],
            });
    }
    Ok(())
}

fn product_counts(pages: &phoenix_memory_contract::GenerationPagesV3) -> [u64; 13] {
    let mut counts = [0_u64; 13];
    counts[(ProducerProductV3::SourceStructure as usize) - 1] = pages
        .sources
        .len()
        .saturating_add(pages.chapters.len())
        .saturating_add(pages.paragraphs.len())
        .saturating_add(pages.sentences.len())
        as u64;
    counts[(ProducerProductV3::ContentUnitsAndChunks as usize) - 1] =
        pages.content_units.len().saturating_add(pages.chunks.len()) as u64;
    counts[(ProducerProductV3::MentionsAndEvidence as usize) - 1] =
        pages.mentions.len().saturating_add(pages.evidence.len()) as u64;
    counts[(ProducerProductV3::CanonicalEntityBindings as usize) - 1] =
        pages.canonical_entity_bindings.len() as u64;
    for candidate in &pages.semantic_candidates {
        if let Some(product) =
            SemanticCandidateFamilyV3::from_raw(candidate.family).map(candidate_product)
        {
            counts[(product as usize) - 1] = counts[(product as usize) - 1].saturating_add(1);
        }
    }
    counts
}

fn source_id(namespace_hash: [u8; 32], kind: SourceKind, external_id: &[u8]) -> u64 {
    deterministic_id(
        match kind {
            SourceKind::WorkspaceDocument => b"source/document",
            SourceKind::Conversation => b"source/conversation",
        },
        &[&namespace_hash, external_id],
    )
}

fn turn_id(
    namespace_hash: [u8; 32],
    source_id: u64,
    conversation_id: u64,
    stored: &StoredTurn,
) -> u64 {
    deterministic_id(
        b"turn",
        &[
            &namespace_hash,
            &source_id.to_le_bytes(),
            &conversation_id.to_le_bytes(),
            &stored.turn.external_id,
        ],
    )
}

fn structural_id(domain: &[u8], source_id: u64, ordinal: usize, content_hash: u64) -> u64 {
    deterministic_id(
        domain,
        &[
            &source_id.to_le_bytes(),
            &(ordinal as u64).to_le_bytes(),
            &content_hash.to_le_bytes(),
        ],
    )
}

fn append_string(slab: &mut Vec<u8>, bytes: &[u8]) -> Result<StringRef, CoordinatorError> {
    let offset = slab.len() as u64;
    let length = u32::try_from(bytes.len()).map_err(|_| CoordinatorError::Oversized)?;
    slab.extend_from_slice(bytes);
    Ok(StringRef {
        offset,
        length,
        reserved: 0,
    })
}
