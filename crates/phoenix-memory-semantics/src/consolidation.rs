use crate::{
    core_pack, CandidateBuilder, CoreRelation, PackDescriptor, SemanticError, VocabularyRelation,
};
use hashbrown::HashMap;
use phoenix_memory_contract::{CandidateEndpointRoleV3, SemanticCandidateFamilyV3};
use phoenix_memory_coordinator::{CandidateEndpointDraft, SemanticCandidateDraft};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct ConsolidationObservation {
    pub source_id: u64,
    pub pack: PackDescriptor,
    pub family: SemanticCandidateFamilyV3,
    pub relation_kind: Arc<str>,
    pub endpoints: Arc<[CandidateEndpointDraft]>,
    pub value: Arc<str>,
    pub evidence_ids: Arc<[u64]>,
    pub confidence: f32,
    pub event_time_millis: i64,
    pub stable_identity_key: Option<[u8; 32]>,
    pub supersedes_candidate: Option<[u8; 32]>,
    pub summary_scope_id: Option<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConsolidationProposal {
    DuplicateIdentity,
    RepeatedFact,
    Conflict,
    Correction,
    ScopedSummary,
}

#[derive(Clone, Debug)]
pub struct ConsolidationReport {
    pub kinds: Arc<[ConsolidationProposal]>,
    pub candidates: Arc<[SemanticCandidateDraft]>,
}

#[derive(Default)]
pub struct ConsolidationEngine;

impl ConsolidationEngine {
    pub fn propose(
        &self,
        observations: &[ConsolidationObservation],
    ) -> Result<ConsolidationReport, SemanticError> {
        let mut output = Vec::new();
        let mut kinds = Vec::new();
        self.propose_identity_duplicates(observations, &mut output, &mut kinds)?;
        self.propose_repetition_and_conflicts(observations, &mut output, &mut kinds)?;
        self.propose_corrections(observations, &mut output, &mut kinds)?;
        self.propose_scoped_summaries(observations, &mut output, &mut kinds)?;
        let mut paired = output.into_iter().zip(kinds).collect::<Vec<_>>();
        paired.sort_unstable_by_key(|(candidate, _)| candidate.candidate_id);
        paired.dedup_by_key(|(candidate, _)| candidate.candidate_id);
        let (candidates, kinds): (Vec<_>, Vec<_>) = paired.into_iter().unzip();
        Ok(ConsolidationReport {
            kinds: kinds.into(),
            candidates: candidates.into(),
        })
    }

    fn propose_identity_duplicates(
        &self,
        observations: &[ConsolidationObservation],
        output: &mut Vec<SemanticCandidateDraft>,
        kinds: &mut Vec<ConsolidationProposal>,
    ) -> Result<(), SemanticError> {
        let mut groups = HashMap::<[u8; 32], Vec<&ConsolidationObservation>>::new();
        for observation in observations {
            if let Some(key) = observation.stable_identity_key {
                groups.entry(key).or_default().push(observation);
            }
        }
        for group in groups.into_values().filter(|group| group.len() > 1) {
            let mut endpoints = Vec::new();
            let mut evidence = Vec::new();
            for observation in group {
                endpoints.extend_from_slice(&observation.endpoints);
                evidence.extend_from_slice(&observation.evidence_ids);
            }
            endpoints.sort_unstable_by_key(|endpoint| endpoint.endpoint_id);
            endpoints.dedup_by_key(|endpoint| endpoint.endpoint_id);
            evidence.sort_unstable();
            evidence.dedup();
            let mut builder = CandidateBuilder::new(
                core_pack(),
                SemanticCandidateFamilyV3::Identity,
                CoreRelation::IdentityAlias.stable_name(),
            );
            for endpoint in endpoints {
                builder = builder.endpoint(endpoint.endpoint_id, CandidateEndpointRoleV3::Subject);
            }
            for evidence_id in evidence {
                builder = builder.evidence(evidence_id);
            }
            output.push(builder.value("explicit-stable-identity-key").build()?);
            kinds.push(ConsolidationProposal::DuplicateIdentity);
        }
        Ok(())
    }

    fn propose_repetition_and_conflicts(
        &self,
        observations: &[ConsolidationObservation],
        output: &mut Vec<SemanticCandidateDraft>,
        kinds: &mut Vec<ConsolidationProposal>,
    ) -> Result<(), SemanticError> {
        type SubjectRelation = (u64, Arc<str>);
        let mut groups = HashMap::<SubjectRelation, Vec<&ConsolidationObservation>>::new();
        for observation in observations {
            if let Some(subject) = observation.endpoints.first() {
                groups
                    .entry((subject.endpoint_id, observation.relation_kind.clone()))
                    .or_default()
                    .push(observation);
            }
        }
        for ((subject, relation), group) in groups {
            let mut values = HashMap::<Arc<str>, Vec<&ConsolidationObservation>>::new();
            for observation in group {
                values
                    .entry(observation.value.clone())
                    .or_default()
                    .push(observation);
            }
            for repeated in values.values().filter(|items| items.len() > 1) {
                output.push(build_group_candidate(
                    CoreRelation::RepeatedEvidence,
                    SemanticCandidateFamilyV3::ContextualEvidence,
                    subject,
                    relation.as_ref(),
                    repeated,
                )?);
                kinds.push(ConsolidationProposal::RepeatedFact);
            }
            if values.len() > 1 {
                let all = values.into_values().flatten().collect::<Vec<_>>();
                output.push(build_group_candidate(
                    CoreRelation::Conflict,
                    SemanticCandidateFamilyV3::Correction,
                    subject,
                    relation.as_ref(),
                    &all,
                )?);
                kinds.push(ConsolidationProposal::Conflict);
            }
        }
        Ok(())
    }

    fn propose_corrections(
        &self,
        observations: &[ConsolidationObservation],
        output: &mut Vec<SemanticCandidateDraft>,
        kinds: &mut Vec<ConsolidationProposal>,
    ) -> Result<(), SemanticError> {
        for observation in observations
            .iter()
            .filter(|item| item.supersedes_candidate.is_some())
        {
            let mut builder = CandidateBuilder::new(
                core_pack(),
                SemanticCandidateFamilyV3::Supersession,
                CoreRelation::Supersession.stable_name(),
            )
            .value(hex_id(observation.supersedes_candidate.unwrap_or([0; 32])));
            for endpoint in observation.endpoints.iter() {
                builder = builder.endpoint(endpoint.endpoint_id, endpoint.role);
            }
            for evidence_id in observation.evidence_ids.iter() {
                builder = builder.evidence(*evidence_id);
            }
            output.push(builder.build()?);
            kinds.push(ConsolidationProposal::Correction);
        }
        Ok(())
    }

    fn propose_scoped_summaries(
        &self,
        observations: &[ConsolidationObservation],
        output: &mut Vec<SemanticCandidateDraft>,
        kinds: &mut Vec<ConsolidationProposal>,
    ) -> Result<(), SemanticError> {
        let mut scopes = HashMap::<(u64, u64), Vec<&ConsolidationObservation>>::new();
        for observation in observations {
            if let Some(scope) = observation.summary_scope_id {
                scopes
                    .entry((observation.pack.id, scope))
                    .or_default()
                    .push(observation);
            }
        }
        for ((_, scope), group) in scopes.into_iter().filter(|(_, group)| group.len() > 1) {
            let pack = group[0].pack;
            let mut builder = CandidateBuilder::new(
                pack,
                SemanticCandidateFamilyV3::ContextualEvidence,
                "lens.scoped_summary",
            )
            .endpoint(scope, CandidateEndpointRoleV3::Context)
            .value(format!("{} evidence-bound observations", group.len()));
            let mut evidence = group
                .iter()
                .flat_map(|item| item.evidence_ids.iter().copied())
                .collect::<Vec<_>>();
            evidence.sort_unstable();
            evidence.dedup();
            for evidence_id in evidence {
                builder = builder.evidence(evidence_id);
            }
            output.push(builder.build()?);
            kinds.push(ConsolidationProposal::ScopedSummary);
        }
        Ok(())
    }
}

fn build_group_candidate(
    relation: CoreRelation,
    family: SemanticCandidateFamilyV3,
    subject: u64,
    source_relation: &str,
    observations: &[&ConsolidationObservation],
) -> Result<SemanticCandidateDraft, SemanticError> {
    let mut builder = CandidateBuilder::new(core_pack(), family, relation.stable_name())
        .endpoint(subject, CandidateEndpointRoleV3::Subject)
        .value(source_relation);
    let mut evidence = observations
        .iter()
        .flat_map(|item| item.evidence_ids.iter().copied())
        .collect::<Vec<_>>();
    evidence.sort_unstable();
    evidence.dedup();
    for evidence_id in evidence {
        builder = builder.evidence(evidence_id);
    }
    builder.build()
}

fn hex_id(id: [u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    for byte in id {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}
