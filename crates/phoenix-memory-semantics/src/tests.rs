use crate::{
    conversation_pack, core_pack, document_pack, narrative_pack, CandidateBuilder,
    ConsolidationEngine, ConsolidationObservation, ConsolidationProposal, ConversationRelation,
    CoreRelation, DeterministicAdjudicatorV1, DocumentRelation, LensSet, MemoryActionV1,
    MemoryEventV1, NarrativeRelation, NliRelationV1, PolicyReasonV1, ScopeRelationV1,
    SemanticAdjudicationInputV1, SourceAuthorityV1, TemporalRelationV1, VocabularyRelation,
    CUE_EXPLICIT_CORRECTION, CUE_TEMPORAL_QUALIFIER,
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

#[test]
fn authoritative_explicit_correction_closes_and_supersedes_without_deleting_history() {
    let input = adjudication_input(
        MemoryEventV1::ExplicitCorrection,
        NliRelationV1::Contradiction,
        ScopeRelationV1::Same,
        TemporalRelationV1::CurrentOverCurrent,
        SourceAuthorityV1::SubjectExplicit,
        CUE_EXPLICIT_CORRECTION,
    );
    let proposal = DeterministicAdjudicatorV1::default()
        .adjudicate(input)
        .expect("valid model lanes");
    assert_eq!(proposal.action, MemoryActionV1::Supersede);
    assert_eq!(proposal.reason, PolicyReasonV1::AuthoritativeCorrection);
    assert!(proposal.close_existing_validity);
    assert!(proposal.preserve_history);
    assert!(proposal.requires_explicit_decision);
}

#[test]
fn later_state_and_distinct_scope_do_not_collapse_into_one_conflict_rule() {
    let later = DeterministicAdjudicatorV1::default()
        .adjudicate(adjudication_input(
            MemoryEventV1::TemporalUpdate,
            NliRelationV1::Contradiction,
            ScopeRelationV1::Same,
            TemporalRelationV1::LaterState,
            SourceAuthorityV1::SubjectExplicit,
            CUE_TEMPORAL_QUALIFIER,
        ))
        .expect("later state");
    assert_eq!(later.action, MemoryActionV1::CloseAndReplace);
    assert!(later.close_existing_validity);

    let scoped = DeterministicAdjudicatorV1::default()
        .adjudicate(adjudication_input(
            MemoryEventV1::ScopeUpdate,
            NliRelationV1::Contradiction,
            ScopeRelationV1::Different,
            TemporalRelationV1::CurrentOverCurrent,
            SourceAuthorityV1::SubjectExplicit,
            0,
        ))
        .expect("distinct scope");
    assert_eq!(scoped.action, MemoryActionV1::RetainBoth);
    assert!(!scoped.close_existing_validity);
}

#[test]
fn model_lanes_are_not_interchangeable_and_low_confidence_never_mutates_truth() {
    let adjudicator = DeterministicAdjudicatorV1::default();
    let mut input = adjudication_input(
        MemoryEventV1::Corroboration,
        NliRelationV1::Entailment,
        ScopeRelationV1::Same,
        TemporalRelationV1::CurrentOverCurrent,
        SourceAuthorityV1::PinnedResult,
        0,
    );
    input.gliclass_role = phoenix_memory_contract::ModelSemanticRoleV3::DedicatedNliObserver;
    assert_eq!(
        adjudicator.adjudicate(input),
        Err(crate::AdjudicationError::InvalidGliclassLane)
    );

    input = adjudication_input(
        MemoryEventV1::Corroboration,
        NliRelationV1::Entailment,
        ScopeRelationV1::Same,
        TemporalRelationV1::CurrentOverCurrent,
        SourceAuthorityV1::PinnedResult,
        0,
    );
    input.modernbert_role = phoenix_memory_contract::ModelSemanticRoleV3::SteerableSemanticObserver;
    assert_eq!(
        adjudicator.adjudicate(input),
        Err(crate::AdjudicationError::InvalidModernbertLane)
    );

    input = adjudication_input(
        MemoryEventV1::ExplicitCorrection,
        NliRelationV1::Contradiction,
        ScopeRelationV1::Same,
        TemporalRelationV1::CurrentOverCurrent,
        SourceAuthorityV1::SubjectExplicit,
        CUE_EXPLICIT_CORRECTION,
    );
    input.memory_event_score = 0.5;
    let deferred = adjudicator.adjudicate(input).expect("bounded scores");
    assert_eq!(deferred.action, MemoryActionV1::Defer);
    assert_eq!(deferred.reason, PolicyReasonV1::InsufficientConfidence);

    let first_identity = input.observation_identity();
    input.nli_relation = NliRelationV1::Neutral;
    assert_ne!(first_identity, input.observation_identity());
}

#[test]
fn hypothetical_memory_remains_candidate_only_and_conflict_requires_review() {
    let adjudicator = DeterministicAdjudicatorV1::default();
    let hypothetical = adjudicator
        .adjudicate(adjudication_input(
            MemoryEventV1::HypotheticalStatement,
            NliRelationV1::Neutral,
            ScopeRelationV1::Same,
            TemporalRelationV1::FutureState,
            SourceAuthorityV1::SubjectExplicit,
            0,
        ))
        .expect("hypothetical");
    assert_eq!(hypothetical.action, MemoryActionV1::CandidateOnly);

    let conflict = adjudicator
        .adjudicate(adjudication_input(
            MemoryEventV1::HardConflict,
            NliRelationV1::Contradiction,
            ScopeRelationV1::Same,
            TemporalRelationV1::CurrentOverCurrent,
            SourceAuthorityV1::Inferred,
            0,
        ))
        .expect("conflict");
    assert_eq!(conflict.action, MemoryActionV1::OpenDispute);
    assert!(conflict.requires_explicit_decision);
}

fn adjudication_input(
    memory_event: MemoryEventV1,
    nli_relation: NliRelationV1,
    scope_relation: ScopeRelationV1,
    temporal_relation: TemporalRelationV1,
    source_authority: SourceAuthorityV1,
    semantic_cues: u32,
) -> SemanticAdjudicationInputV1 {
    SemanticAdjudicationInputV1 {
        memory_event,
        memory_event_score: 0.95,
        nli_relation,
        nli_score: 0.95,
        scope_relation,
        temporal_relation,
        source_authority,
        semantic_cues,
        evidence_count: 2,
        gliclass_role: phoenix_memory_contract::ModelSemanticRoleV3::SteerableSemanticObserver,
        modernbert_role: phoenix_memory_contract::ModelSemanticRoleV3::DedicatedNliObserver,
    }
}
