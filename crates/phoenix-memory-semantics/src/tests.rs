use crate::{
    conversation_pack, core_pack, document_pack, narrative_pack, CandidateBuilder,
    ConsolidationEngine, ConsolidationObservation, ConsolidationProposal, ConversationRelation,
    CoreRelation, DocumentRelation, LensSet, NarrativeRelation, VocabularyRelation,
};
use phoenix_memory_contract::{
    CandidateEndpointRoleV3, CandidateStatus, SemanticCandidateFamilyV3,
};
use phoenix_memory_coordinator::CandidateEndpointDraft;
use std::sync::Arc;

#[test]
fn narrative_document_and_conversation_share_core_without_type_leakage() {
    let evidence = 9001;
    let core = CandidateBuilder::new(
        core_pack(),
        SemanticCandidateFamilyV3::Attribute,
        CoreRelation::Attribute.stable_name(),
    )
    .endpoint(10, CandidateEndpointRoleV3::Subject)
    .evidence(evidence)
    .value("green")
    .build()
    .expect("core candidate");
    let narrative = CandidateBuilder::relation(
        narrative_pack(),
        SemanticCandidateFamilyV3::Event,
        NarrativeRelation::SceneMembership,
    )
    .endpoint(10, CandidateEndpointRoleV3::Participant)
    .endpoint(11, CandidateEndpointRoleV3::Context)
    .evidence(evidence)
    .build()
    .expect("narrative candidate");
    let conversation = CandidateBuilder::relation(
        conversation_pack(),
        SemanticCandidateFamilyV3::Goal,
        ConversationRelation::Commitment,
    )
    .endpoint(10, CandidateEndpointRoleV3::Subject)
    .evidence(evidence)
    .build()
    .expect("conversation candidate");
    let document = CandidateBuilder::relation(
        document_pack(),
        SemanticCandidateFamilyV3::Claim,
        DocumentRelation::Definition,
    )
    .endpoint(10, CandidateEndpointRoleV3::Subject)
    .evidence(evidence)
    .build()
    .expect("document candidate");

    assert_eq!(core.status, CandidateStatus::Proposed);
    assert_eq!(narrative.status, CandidateStatus::Proposed);
    assert_eq!(conversation.status, CandidateStatus::Proposed);
    assert_eq!(document.status, CandidateStatus::Proposed);
    assert_ne!(core.vocabulary_pack_id, narrative.vocabulary_pack_id);
    assert_ne!(conversation.vocabulary_pack_id, document.vocabulary_pack_id);
}

#[test]
fn lens_removal_does_not_change_core_candidate_identity() {
    let build = || {
        CandidateBuilder::new(
            core_pack(),
            SemanticCandidateFamilyV3::Belief,
            CoreRelation::Belief.stable_name(),
        )
        .endpoint(42, CandidateEndpointRoleV3::Subject)
        .evidence(7)
        .value("bounded memory")
        .build()
        .expect("core candidate")
    };
    let with_all_lenses = (LensSet::ALL, build());
    let core_only = (LensSet::NONE, build());
    assert!(with_all_lenses.0.contains(LensSet::NARRATIVE));
    assert_eq!(with_all_lenses.1.candidate_id, core_only.1.candidate_id);
}

#[test]
fn candidate_identity_is_canonical_but_time_and_model_sensitive() {
    let build = |reverse: bool, valid_from: i64, model: u32| {
        let builder = CandidateBuilder::relation(
            core_pack(),
            SemanticCandidateFamilyV3::Relationship,
            CoreRelation::Relationship,
        )
        .valid_time(valid_from, valid_from + 100)
        .model_identity(model);
        let builder = if reverse {
            builder
                .endpoint(22, CandidateEndpointRoleV3::Object)
                .endpoint(11, CandidateEndpointRoleV3::Subject)
                .evidence(8)
                .evidence(7)
        } else {
            builder
                .endpoint(11, CandidateEndpointRoleV3::Subject)
                .endpoint(22, CandidateEndpointRoleV3::Object)
                .evidence(7)
                .evidence(8)
        };
        builder.build().expect("canonical candidate")
    };

    let canonical = build(false, 1_000, 3);
    assert_eq!(canonical.candidate_id, build(true, 1_000, 3).candidate_id);
    assert_ne!(canonical.candidate_id, build(false, 2_000, 3).candidate_id);
    assert_ne!(canonical.candidate_id, build(false, 1_000, 4).candidate_id);
}

#[test]
fn consolidation_proposes_repetition_conflict_correction_and_summary() {
    let pack = conversation_pack();
    let endpoint: Arc<[CandidateEndpointDraft]> = Arc::from([CandidateEndpointDraft {
        endpoint_id: 55,
        role: CandidateEndpointRoleV3::Subject,
        flags: 0,
    }]);
    let base = |value: &'static str, evidence: u64, supersedes_candidate: Option<[u8; 32]>| {
        ConsolidationObservation {
            source_id: evidence,
            pack,
            family: SemanticCandidateFamilyV3::Attribute,
            relation_kind: Arc::from("conversation.preference"),
            endpoints: endpoint.clone(),
            value: Arc::from(value),
            evidence_ids: Arc::from([evidence]),
            confidence: 0.8,
            event_time_millis: evidence as i64,
            stable_identity_key: None,
            supersedes_candidate,
            summary_scope_id: Some(88),
        }
    };
    let observations = [
        base("tea", 1, None),
        base("tea", 2, None),
        base("coffee", 3, Some([4; 32])),
    ];
    let report = ConsolidationEngine
        .propose(&observations)
        .expect("consolidation proposals");

    assert!(report.kinds.contains(&ConsolidationProposal::RepeatedFact));
    assert!(report.kinds.contains(&ConsolidationProposal::Conflict));
    assert!(report.kinds.contains(&ConsolidationProposal::Correction));
    assert!(report.kinds.contains(&ConsolidationProposal::ScopedSummary));
    assert!(report
        .candidates
        .iter()
        .all(|candidate| candidate.status == CandidateStatus::Proposed
            && !candidate.evidence_ids.is_empty()));
}

#[test]
fn duplicate_identity_requires_explicit_stable_key_not_label() {
    let pack = core_pack();
    let observation = |entity: u64, evidence: u64, key| ConsolidationObservation {
        source_id: evidence,
        pack,
        family: SemanticCandidateFamilyV3::Identity,
        relation_kind: Arc::from("core.identity_alias"),
        endpoints: Arc::from([CandidateEndpointDraft {
            endpoint_id: entity,
            role: CandidateEndpointRoleV3::Subject,
            flags: 0,
        }]),
        value: Arc::from("same display label"),
        evidence_ids: Arc::from([evidence]),
        confidence: 1.0,
        event_time_millis: 0,
        stable_identity_key: key,
        supersedes_candidate: None,
        summary_scope_id: None,
    };
    let without_key = ConsolidationEngine
        .propose(&[observation(1, 10, None), observation(2, 11, None)])
        .expect("label-only observations");
    assert!(!without_key
        .kinds
        .contains(&ConsolidationProposal::DuplicateIdentity));

    let with_key = ConsolidationEngine
        .propose(&[
            observation(1, 10, Some([7; 32])),
            observation(2, 11, Some([7; 32])),
        ])
        .expect("stable-key observations");
    assert!(with_key
        .kinds
        .contains(&ConsolidationProposal::DuplicateIdentity));
}
