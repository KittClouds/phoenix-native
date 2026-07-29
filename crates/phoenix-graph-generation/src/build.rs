use crate::error::GraphGenerationError;
use crate::format::*;
use bytemuck::{bytes_of, cast_slice, Pod};
use phoenix_analysis_contract::{
    AnalysisModelIdentity, NliCandidateKind, NliDecision, PhoenixDocumentAnalysisV1,
    PhoenixStructuralSubstrateV1,
};
use std::collections::BTreeSet;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug)]
pub struct AcceptedEdgeInput {
    pub id: u64,
    pub source_id: u64,
    pub target_id: u64,
    pub evidence_id: u64,
    pub weight: f32,
    pub relation: u16,
    pub flags: u16,
}

#[derive(Clone, Debug)]
pub struct CanonicalEntityInput<'a> {
    pub id: u64,
    pub label: &'a str,
    pub custom_kind: Option<&'a str>,
    pub mention_count: u32,
    pub kind: u16,
    pub source_mask: u16,
}

#[derive(Clone, Debug)]
pub struct DurableDecisionInput<'a> {
    pub id: u64,
    pub candidate_id: [u8; 32],
    pub reason: &'a str,
    pub decided_at_revision: u64,
    pub status: u16,
    pub flags: u16,
}

#[derive(Clone, Debug)]
pub struct ProducerCapabilityInput<'a> {
    pub name: &'a str,
    pub producer: &'a str,
    pub supported: bool,
    pub emitted: bool,
    pub flags: u32,
}

pub struct GraphGenerationInput<'a> {
    pub text: &'a str,
    pub analysis: &'a PhoenixDocumentAnalysisV1,
    pub structural: &'a PhoenixStructuralSubstrateV1,
    pub canonical_entities: &'a [CanonicalEntityInput<'a>],
    pub accepted_edges: &'a [AcceptedEdgeInput],
    pub decisions: &'a [DurableDecisionInput<'a>],
    pub capabilities: &'a [ProducerCapabilityInput<'a>],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GraphGenerationWriteReceipt {
    pub generation_hash: [u8; 32],
    pub byte_len: u64,
    pub chunk_count: u32,
    pub mention_count: u32,
}

pub fn write_graph_generation_new(
    path: impl AsRef<Path>,
    input: &GraphGenerationInput<'_>,
) -> Result<GraphGenerationWriteReceipt, GraphGenerationError> {
    validate_input(input)?;
    let built = Builder::new(input)?.finish()?;
    let path = path.as_ref();
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|source| match source.kind() {
            std::io::ErrorKind::AlreadyExists => {
                GraphGenerationError::AlreadyExists(path.to_path_buf())
            }
            _ => GraphGenerationError::io(path, source),
        })?;
    file.write_all(&built.bytes)
        .and_then(|_| file.sync_all())
        .map_err(|source| GraphGenerationError::io(path, source))?;
    Ok(GraphGenerationWriteReceipt {
        generation_hash: built.generation_hash,
        byte_len: built.bytes.len() as u64,
        chunk_count: input.structural.chunks.len() as u32,
        mention_count: input.analysis.ner.mentions.len() as u32,
    })
}

struct BuiltGeneration {
    bytes: Vec<u8>,
    generation_hash: [u8; 32],
}

struct Builder<'a> {
    input: &'a GraphGenerationInput<'a>,
    strings: StringSlab,
    document: Vec<DocumentRecord>,
    chunks: Vec<ChunkRecord>,
    sentences: Vec<SentenceRecord>,
    spans: Vec<SpanRecord>,
    entities: Vec<EntityRecord>,
    mentions: Vec<MentionRecord>,
    evidence: Vec<EvidenceRecord>,
    accepted: Vec<AcceptedEdgeRecord>,
    candidates: Vec<CandidateEdgeRecord>,
    adjudications: Vec<AdjudicationRecord>,
    decisions: Vec<DecisionRecord>,
    capabilities: Vec<CapabilityRecord>,
    identities: Vec<IdentityRecord>,
    stages: Vec<StageReceiptRecord>,
}

impl<'a> Builder<'a> {
    fn new(input: &'a GraphGenerationInput<'a>) -> Result<Self, GraphGenerationError> {
        let mut builder = Self {
            input,
            strings: StringSlab::default(),
            document: Vec::with_capacity(1),
            chunks: Vec::with_capacity(input.structural.chunks.len()),
            sentences: Vec::with_capacity(input.structural.sentences.len()),
            spans: Vec::with_capacity(input.structural.spans.len()),
            entities: Vec::with_capacity(input.canonical_entities.len()),
            mentions: Vec::with_capacity(input.analysis.ner.mentions.len()),
            evidence: Vec::with_capacity(input.analysis.ner.mentions.len()),
            accepted: Vec::with_capacity(
                input.accepted_edges.len()
                    + input.structural.chunks.len()
                    + input.analysis.ner.mentions.len() * 2,
            ),
            candidates: Vec::with_capacity(input.analysis.nli.nli_candidates.len()),
            adjudications: Vec::with_capacity(input.analysis.nli.nli_adjudications.len()),
            decisions: Vec::with_capacity(input.decisions.len()),
            capabilities: Vec::with_capacity(input.capabilities.len()),
            identities: Vec::with_capacity(4),
            stages: Vec::with_capacity(4),
        };
        builder.build()?;
        Ok(builder)
    }

    fn build(&mut self) -> Result<(), GraphGenerationError> {
        let binding = &self.input.analysis.ner.binding;
        let document_id = stable_id(
            b"document",
            &binding.content_hash,
            &[binding.native_document_id],
        );
        let source_id = self.strings.push(&binding.source_document_id)?;
        self.build_structure(document_id)?;
        self.build_entities_and_mentions()?;
        self.build_nli();
        self.build_decisions_and_capabilities()?;
        self.build_identities_and_stages()?;
        self.document.push(DocumentRecord {
            id: document_id,
            source_id,
            source_len: self.input.structural.source_len,
            chunk_count: checked_len(self.chunks.len(), "chunk count")?,
            sentence_count: checked_len(self.sentences.len(), "sentence count")?,
            span_count: checked_len(self.spans.len(), "span count")?,
            entity_count: checked_len(self.entities.len(), "entity count")?,
            mention_count: checked_len(self.mentions.len(), "mention count")?,
            accepted_edge_count: checked_len(self.accepted.len(), "accepted edge count")?,
            candidate_edge_count: checked_len(self.candidates.len(), "candidate edge count")?,
        });
        self.validate_built_authority()?;
        Ok(())
    }

    fn validate_built_authority(&self) -> Result<(), GraphGenerationError> {
        let mut node_ids = BTreeSet::new();
        node_ids.insert(self.document[0].id);
        for id in self
            .chunks
            .iter()
            .map(|record| record.id)
            .chain(self.sentences.iter().map(|record| record.id))
            .chain(self.spans.iter().map(|record| record.id))
            .chain(self.entities.iter().map(|record| record.id))
            .chain(self.evidence.iter().map(|record| record.id))
        {
            if id == 0 || !node_ids.insert(id) {
                return Err(GraphGenerationError::Binding("generation node IDs collide"));
            }
        }
        let evidence_ids = self
            .evidence
            .iter()
            .map(|record| record.id)
            .collect::<BTreeSet<_>>();
        let mut accepted_ids = BTreeSet::new();
        if self.accepted.iter().any(|record| {
            record.id == 0
                || !accepted_ids.insert(record.id)
                || !node_ids.contains(&record.source_id)
                || !node_ids.contains(&record.target_id)
                || record.source_id == record.target_id
                || (record.evidence_id != 0 && !evidence_ids.contains(&record.evidence_id))
        }) {
            return Err(GraphGenerationError::Binding(
                "accepted structural edge authority is invalid",
            ));
        }
        Ok(())
    }

    fn build_structure(&mut self, document_id: u64) -> Result<(), GraphGenerationError> {
        let hash = &self.input.analysis.ner.binding.content_hash;
        for (ordinal, chunk) in self.input.structural.chunks.iter().enumerate() {
            let id = stable_id(
                b"chunk",
                hash,
                &[
                    ordinal as u64,
                    u64::from(chunk.start),
                    u64::from(chunk.end),
                    chunk.content_hash,
                ],
            );
            self.chunks.push(ChunkRecord {
                id,
                content_hash: chunk.content_hash,
                start: chunk.start,
                end: chunk.end,
                sentence_start: chunk.sentence_start,
                sentence_end: chunk.sentence_end,
                paragraph_start: chunk.paragraph_start,
                paragraph_end: chunk.paragraph_end,
                chapter_index: chunk.chapter_index,
                token_count: chunk.token_count,
            });
            self.accepted.push(AcceptedEdgeRecord {
                id: stable_id(b"document-chunk", hash, &[document_id, id]),
                source_id: document_id,
                target_id: id,
                evidence_id: 0,
                weight_bits: 1.0_f32.to_bits(),
                relation: 1,
                flags: 1,
            });
        }
        for (ordinal, sentence) in self.input.structural.sentences.iter().enumerate() {
            self.sentences.push(SentenceRecord {
                id: stable_id(
                    b"sentence",
                    hash,
                    &[
                        ordinal as u64,
                        u64::from(sentence.start),
                        u64::from(sentence.end),
                        sentence.content_hash,
                    ],
                ),
                content_hash: sentence.content_hash,
                start: sentence.start,
                end: sentence.end,
                paragraph_index: sentence.paragraph_index,
                chapter_index: sentence.chapter_index,
                token_count: sentence.token_count,
                quality: sentence.quality as u16,
                dialogue_hint: sentence.dialogue_hint as u16,
            });
        }
        for (ordinal, span) in self.input.structural.spans.iter().enumerate() {
            self.spans.push(SpanRecord {
                id: stable_id(
                    b"span",
                    hash,
                    &[
                        ordinal as u64,
                        u64::from(span.start),
                        u64::from(span.end),
                        span.content_hash,
                    ],
                ),
                content_hash: span.content_hash,
                label: self.strings.push(&span.label)?,
                start: span.start,
                end: span.end,
                parent_index: span.parent_index,
                child_start: span.child_start,
                child_end: span.child_end,
                token_count: span.token_count,
                kind: span.kind as u16,
                dialogue_hint: span.dialogue_hint as u16,
                reserved: 0,
            });
        }
        for edge in self.input.accepted_edges {
            self.accepted.push(AcceptedEdgeRecord {
                id: edge.id,
                source_id: edge.source_id,
                target_id: edge.target_id,
                evidence_id: edge.evidence_id,
                weight_bits: edge.weight.to_bits(),
                relation: edge.relation,
                flags: edge.flags,
            });
        }
        Ok(())
    }

    fn build_entities_and_mentions(&mut self) -> Result<(), GraphGenerationError> {
        let hash = &self.input.analysis.ner.binding.content_hash;
        for entity in self.input.canonical_entities {
            self.entities.push(EntityRecord {
                id: entity.id,
                label: self.strings.push(entity.label)?,
                custom_kind: self.strings.push(entity.custom_kind.unwrap_or_default())?,
                mention_count: entity.mention_count,
                kind: entity.kind,
                source_mask: entity.source_mask,
            });
        }
        for mention in &self.input.analysis.ner.mentions {
            let chunk = containing_chunk(&self.chunks, mention.start, mention.end)?;
            let evidence_id = stable_id(
                b"evidence",
                hash,
                &[
                    mention.entity_id,
                    u64::from(mention.start),
                    u64::from(mention.end),
                    mention.mention_id,
                ],
            );
            self.mentions.push(MentionRecord {
                id: mention.mention_id,
                entity_id: mention.entity_id,
                evidence_id,
                chunk_id: chunk.id,
                start: mention.start,
                end: mention.end,
                sentence_index: mention.sentence_index,
                confidence_bits: mention.confidence.to_bits(),
                flags: u32::from(mention.accepted),
                reserved: 0,
            });
            self.evidence.push(EvidenceRecord {
                id: evidence_id,
                entity_id: mention.entity_id,
                chunk_id: chunk.id,
                start: mention.start,
                end: mention.end,
            });
            self.accepted.extend([
                AcceptedEdgeRecord {
                    id: stable_id(b"chunk-evidence", hash, &[chunk.id, evidence_id]),
                    source_id: chunk.id,
                    target_id: evidence_id,
                    evidence_id,
                    weight_bits: mention.confidence.to_bits(),
                    relation: 2,
                    flags: 1,
                },
                AcceptedEdgeRecord {
                    id: stable_id(b"evidence-entity", hash, &[evidence_id, mention.entity_id]),
                    source_id: evidence_id,
                    target_id: mention.entity_id,
                    evidence_id,
                    weight_bits: mention.confidence.to_bits(),
                    relation: 3,
                    flags: 1,
                },
            ]);
        }
        Ok(())
    }

    fn build_nli(&mut self) {
        for candidate in &self.input.analysis.nli.nli_candidates {
            self.candidates.push(CandidateEdgeRecord {
                candidate_id: candidate.candidate_id,
                source_id: candidate.left_entity_id,
                target_id: candidate.right_entity_id,
                premise_start: candidate.premise_start,
                premise_end: candidate.premise_end,
                relation: candidate_kind(candidate.kind),
                flags: 1,
                reserved: 0,
            });
        }
        for adjudication in &self.input.analysis.nli.nli_adjudications {
            self.adjudications.push(AdjudicationRecord {
                candidate_id: adjudication.candidate_id,
                contradiction_bits: adjudication.contradiction_millis,
                entailment_bits: adjudication.entailment_millis,
                neutral_bits: adjudication.neutral_millis,
                label: decision(adjudication.decision),
                flags: u16::from(adjudication.needs_human_review),
            });
        }
    }

    fn build_decisions_and_capabilities(&mut self) -> Result<(), GraphGenerationError> {
        for decision in self.input.decisions {
            self.decisions.push(DecisionRecord {
                id: decision.id,
                candidate_id: decision.candidate_id,
                reason: self.strings.push(decision.reason)?,
                decided_at_revision: decision.decided_at_revision,
                status: decision.status,
                flags: decision.flags,
                reserved: 0,
            });
        }
        for capability in self.input.capabilities {
            self.capabilities.push(CapabilityRecord {
                name: self.strings.push(capability.name)?,
                producer: self.strings.push(capability.producer)?,
                supported: u16::from(capability.supported),
                emitted: u16::from(capability.emitted),
                flags: capability.flags,
            });
        }
        Ok(())
    }

    fn build_identities_and_stages(&mut self) -> Result<(), GraphGenerationError> {
        let binding = &self.input.analysis.ner.binding;
        self.push_identity(
            "producer-binary",
            "native-executable",
            binding.producer_binary_hash,
            [0; 32],
        )?;
        for identity in [&binding.chunker, &binding.dynamic_ner, &binding.nli] {
            self.push_model_identity(identity)?;
        }
        let receipt = self.input.analysis.ner.receipt;
        let stages = [
            ("chunker", 1, 0, receipt.chunker_micros, receipt.chunk_count),
            (
                "dynamic-ner",
                2,
                1,
                receipt.dynamic_ner_micros,
                receipt.mention_count,
            ),
            ("nli-load", 3, 0, receipt.nli_load_micros, 1),
            (
                "nli-adjudication",
                4,
                3,
                receipt.nli_adjudication_micros,
                receipt.nli_adjudication_count,
            ),
        ];
        for (name, span_id, parent_span_id, elapsed, outputs) in stages {
            self.stages.push(StageReceiptRecord {
                name: self.strings.push(name)?,
                span_id,
                parent_span_id,
                elapsed_micros: elapsed,
                output_count: u64::from(outputs),
                flags: 1,
                reserved: 0,
            });
        }
        Ok(())
    }

    fn push_model_identity(
        &mut self,
        identity: &AnalysisModelIdentity,
    ) -> Result<(), GraphGenerationError> {
        self.push_identity(
            &identity.model_id,
            &identity.runtime_id,
            identity.artifact_hash,
            identity.config_hash,
        )
    }

    fn push_identity(
        &mut self,
        name: &str,
        runtime: &str,
        artifact_hash: [u8; 32],
        config_hash: [u8; 32],
    ) -> Result<(), GraphGenerationError> {
        self.identities.push(IdentityRecord {
            name: self.strings.push(name)?,
            runtime: self.strings.push(runtime)?,
            artifact_hash,
            config_hash,
        });
        Ok(())
    }

    fn finish(self) -> Result<BuiltGeneration, GraphGenerationError> {
        let sections = [
            section(SectionKind::Document, &self.document),
            section(SectionKind::Chunks, &self.chunks),
            section(SectionKind::Sentences, &self.sentences),
            section(SectionKind::Spans, &self.spans),
            raw_section(SectionKind::Strings, self.strings.bytes),
            section(SectionKind::Entities, &self.entities),
            section(SectionKind::Mentions, &self.mentions),
            section(SectionKind::Evidence, &self.evidence),
            section(SectionKind::AcceptedEdges, &self.accepted),
            section(SectionKind::CandidateEdges, &self.candidates),
            section(SectionKind::Adjudications, &self.adjudications),
            section(SectionKind::Decisions, &self.decisions),
            section(SectionKind::Capabilities, &self.capabilities),
            section(SectionKind::Identities, &self.identities),
            section(SectionKind::StageReceipts, &self.stages),
        ];
        assemble(self.input, sections)
    }
}

struct RawSection {
    kind: SectionKind,
    record_size: u32,
    count: u64,
    bytes: Vec<u8>,
}

fn section<T: Pod>(kind: SectionKind, records: &[T]) -> RawSection {
    RawSection {
        kind,
        record_size: std::mem::size_of::<T>() as u32,
        count: records.len() as u64,
        bytes: cast_slice(records).to_vec(),
    }
}

fn raw_section(kind: SectionKind, bytes: Vec<u8>) -> RawSection {
    RawSection {
        kind,
        record_size: 1,
        count: bytes.len() as u64,
        bytes,
    }
}

fn assemble(
    input: &GraphGenerationInput<'_>,
    sections: [RawSection; 15],
) -> Result<BuiltGeneration, GraphGenerationError> {
    let header_len = std::mem::size_of::<GenerationHeader>();
    let directory_len = sections.len() * std::mem::size_of::<SectionDescriptor>();
    let mut cursor = align8(header_len + directory_len);
    let mut descriptors = Vec::with_capacity(sections.len());
    for section in &sections {
        cursor = align8(cursor);
        descriptors.push(SectionDescriptor {
            kind: section.kind as u16,
            flags: 1,
            record_size: section.record_size,
            offset: cursor as u64,
            length: section.bytes.len() as u64,
            count: section.count,
            hash: *blake3::hash(&section.bytes).as_bytes(),
        });
        cursor = cursor
            .checked_add(section.bytes.len())
            .ok_or(GraphGenerationError::Oversized("generation length"))?;
    }
    if cursor as u64 > MAX_GENERATION_BYTES {
        return Err(GraphGenerationError::Oversized("generation bytes"));
    }
    let binding = &input.analysis.ner.binding;
    let mut header = GenerationHeader {
        magic: GRAPH_GENERATION_MAGIC,
        version: GRAPH_GENERATION_VERSION,
        header_size: header_len as u32,
        section_count: sections.len() as u32,
        flags: 1,
        total_len: cursor as u64,
        source_document_id_hash: *blake3::hash(binding.source_document_id.as_bytes()).as_bytes(),
        content_hash: binding.content_hash,
        generation_hash: [0; 32],
        native_document_id: binding.native_document_id,
        document_revision: binding.document_revision,
        registry_revision: binding.target_registry_revision,
        analysis_generation: binding.analysis_generation,
    };
    let mut bytes = vec![0_u8; cursor];
    bytes[..header_len].copy_from_slice(bytes_of(&header));
    bytes[header_len..header_len + directory_len].copy_from_slice(cast_slice(&descriptors));
    for (section, descriptor) in sections.iter().zip(&descriptors) {
        let start = descriptor.offset as usize;
        bytes[start..start + section.bytes.len()].copy_from_slice(&section.bytes);
    }
    let generation_hash = *blake3::hash(&bytes).as_bytes();
    header.generation_hash = generation_hash;
    bytes[..header_len].copy_from_slice(bytes_of(&header));
    Ok(BuiltGeneration {
        bytes,
        generation_hash,
    })
}

#[derive(Default)]
struct StringSlab {
    bytes: Vec<u8>,
}

impl StringSlab {
    fn push(&mut self, value: &str) -> Result<StringRef, GraphGenerationError> {
        let offset = checked_len(self.bytes.len(), "string slab offset")?;
        let length = checked_len(value.len(), "string length")?;
        self.bytes
            .try_reserve(value.len())
            .map_err(|_| GraphGenerationError::Oversized("string slab"))?;
        self.bytes.extend_from_slice(value.as_bytes());
        Ok(StringRef { offset, length })
    }
}

fn validate_input(input: &GraphGenerationInput<'_>) -> Result<(), GraphGenerationError> {
    input
        .analysis
        .validate()
        .map_err(GraphGenerationError::Binding)?;
    input
        .structural
        .validate()
        .map_err(GraphGenerationError::Binding)?;
    let binding = &input.analysis.ner.binding;
    if &input.structural.binding != binding {
        return Err(GraphGenerationError::Binding(
            "analysis and structural sidecar differ",
        ));
    }
    if input.text.len() != input.structural.source_len as usize
        || *blake3::hash(input.text.as_bytes()).as_bytes() != binding.content_hash
    {
        return Err(GraphGenerationError::Binding(
            "source text does not match authority",
        ));
    }
    if input.accepted_edges.len() as u64 > MAX_RECORDS_PER_SECTION
        || input.canonical_entities.is_empty()
        || input.canonical_entities.len() as u64 > MAX_RECORDS_PER_SECTION
        || input.decisions.len() as u64 > MAX_RECORDS_PER_SECTION
        || input.capabilities.len() as u64 > MAX_RECORDS_PER_SECTION
    {
        return Err(GraphGenerationError::Oversized("input record section"));
    }
    if has_duplicate(input.accepted_edges.iter().map(|edge| edge.id))
        || has_duplicate(input.decisions.iter().map(|decision| decision.id))
        || input.accepted_edges.iter().any(|edge| {
            edge.source_id == 0
                || edge.target_id == 0
                || edge.source_id == edge.target_id
                || !edge.weight.is_finite()
        })
        || input.capabilities.iter().any(|capability| {
            capability.name.trim().is_empty() || capability.producer.trim().is_empty()
        })
    {
        return Err(GraphGenerationError::Binding(
            "accepted edge, decision, or capability input is invalid",
        ));
    }
    let candidate_ids = input
        .analysis
        .nli
        .nli_candidates
        .iter()
        .map(|candidate| candidate.candidate_id)
        .collect::<BTreeSet<_>>();
    if input
        .decisions
        .iter()
        .any(|decision| !candidate_ids.contains(&decision.candidate_id))
    {
        return Err(GraphGenerationError::Binding(
            "durable decision references an unknown candidate",
        ));
    }
    validate_review_promotion(input)?;
    let ids = input
        .canonical_entities
        .iter()
        .map(|entity| entity.id)
        .collect::<BTreeSet<_>>();
    if ids.len() != input.canonical_entities.len()
        || input.canonical_entities.iter().any(|entity| {
            entity.id == 0
                || entity.label.trim().is_empty()
                || entity.source_mask == 0
                || entity.mention_count == 0
        })
        || input
            .analysis
            .ner
            .entities
            .iter()
            .any(|entity| !ids.contains(&entity.stable_id))
        || input.analysis.nli.nli_candidates.iter().any(|candidate| {
            !ids.contains(&candidate.left_entity_id) || !ids.contains(&candidate.right_entity_id)
        })
    {
        return Err(GraphGenerationError::Binding(
            "canonical entity authority is incomplete or inconsistent",
        ));
    }
    Ok(())
}

fn validate_review_promotion(input: &GraphGenerationInput<'_>) -> Result<(), GraphGenerationError> {
    let mut decided_candidates = BTreeSet::new();
    if input.decisions.iter().any(|decision| {
        !decided_candidates.insert(decision.candidate_id)
            || !matches!(
                decision.status,
                crate::DECISION_STATUS_ACCEPTED
                    | crate::DECISION_STATUS_REJECTED
                    | crate::DECISION_STATUS_DEFERRED
            )
            || decision.flags & crate::DECISION_FLAG_DURABLE_RECEIPT == 0
    }) {
        return Err(GraphGenerationError::Binding(
            "effective decisions require one durable receipt per candidate",
        ));
    }

    let candidates = input
        .analysis
        .nli
        .nli_candidates
        .iter()
        .map(|candidate| (candidate.candidate_id, candidate))
        .collect::<std::collections::BTreeMap<_, _>>();
    let accepted = input
        .decisions
        .iter()
        .filter(|decision| decision.status == crate::DECISION_STATUS_ACCEPTED)
        .map(|decision| decision.candidate_id)
        .collect::<BTreeSet<_>>();
    let mut promoted = BTreeSet::new();
    for edge in input
        .accepted_edges
        .iter()
        .filter(|edge| edge.flags & crate::ACCEPTED_EDGE_FLAG_PROMOTED != 0)
    {
        let matching = candidates.iter().find_map(|(candidate_id, candidate)| {
            (crate::promoted_edge_id(*candidate_id) == edge.id
                && candidate.left_entity_id == edge.source_id
                && candidate.right_entity_id == edge.target_id
                && candidate_kind(candidate.kind) == edge.relation)
                .then_some(*candidate_id)
        });
        let Some(candidate_id) = matching else {
            return Err(GraphGenerationError::Binding(
                "promoted edge does not match a semantic candidate",
            ));
        };
        if !accepted.contains(&candidate_id) || !promoted.insert(candidate_id) {
            return Err(GraphGenerationError::Binding(
                "promoted edge lacks a unique accepted decision receipt",
            ));
        }
    }
    if promoted != accepted {
        return Err(GraphGenerationError::Binding(
            "accepted decision is missing its promoted edge",
        ));
    }
    Ok(())
}

fn has_duplicate(values: impl Iterator<Item = u64>) -> bool {
    let mut seen = BTreeSet::new();
    values
        .into_iter()
        .any(|value| value == 0 || !seen.insert(value))
}

fn containing_chunk(
    chunks: &[ChunkRecord],
    start: u32,
    end: u32,
) -> Result<&ChunkRecord, GraphGenerationError> {
    chunks
        .iter()
        .enumerate()
        .filter(|(_, chunk)| chunk.start <= start && chunk.end >= end)
        .min_by_key(|(ordinal, chunk)| (chunk.end - chunk.start, *ordinal))
        .map(|(_, chunk)| chunk)
        .ok_or(GraphGenerationError::Binding(
            "mention is not contained by an exact dynamic chunk",
        ))
}

fn stable_id(domain: &[u8], hash: &[u8; 32], values: &[u64]) -> u64 {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"phoenix.graph-generation/v1\0");
    hasher.update(domain);
    hasher.update(&[0]);
    hasher.update(hash);
    for value in values {
        hasher.update(&value.to_le_bytes());
    }
    let mut raw = [0_u8; 8];
    raw.copy_from_slice(&hasher.finalize().as_bytes()[..8]);
    u64::from_le_bytes(raw).max(1)
}

fn candidate_kind(value: NliCandidateKind) -> u16 {
    match value {
        NliCandidateKind::SameSurface => 1,
        NliCandidateKind::Alias => 2,
        NliCandidateKind::Coreference => 3,
        NliCandidateKind::Related => 4,
    }
}

fn decision(value: NliDecision) -> u16 {
    match value {
        NliDecision::Supported => 1,
        NliDecision::Contradicted => 2,
        NliDecision::Unknown => 3,
    }
}

fn checked_len(value: usize, name: &'static str) -> Result<u32, GraphGenerationError> {
    u32::try_from(value).map_err(|_| GraphGenerationError::Oversized(name))
}

fn align8(value: usize) -> usize {
    (value + 7) & !7
}

#[allow(dead_code)]
fn _path_for_generation(root: &Path, generation: u64) -> PathBuf {
    root.join(format!(
        "generation-{generation:020}.{GRAPH_GENERATION_EXTENSION}"
    ))
}
