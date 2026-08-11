use super::*;
use phoenix_lexical_qps::{
    KeyedIdentity, PairwiseJudgmentDraftV3, PairwiseJudgmentV3, RankEvidenceV3, RelevanceTier,
};

fn groups() -> SplitGroupProvenanceV3 {
    SplitGroupProvenanceV3 {
        query_family_identity: KeyedIdentity::from_bytes([1; 32]),
        positive_source_identity: KeyedIdentity::from_bytes([2; 32]),
        negative_source_identity: KeyedIdentity::from_bytes([3; 32]),
        positive_near_duplicate_cluster_identity: KeyedIdentity::from_bytes([4; 32]),
        negative_near_duplicate_cluster_identity: KeyedIdentity::from_bytes([5; 32]),
        entity_or_identifier_family_identity: KeyedIdentity::from_bytes([6; 32]),
        collection_cohort_identity: KeyedIdentity::from_bytes([7; 32]),
        collected_at_unix_seconds: 1,
    }
}

#[test]
fn reversal_swaps_only_directional_provenance() {
    let original = groups();
    let reversed = reversed_groups(original);
    assert_eq!(
        reversed.positive_source_identity,
        original.negative_source_identity
    );
    assert_eq!(
        reversed.negative_source_identity,
        original.positive_source_identity
    );
    assert_eq!(
        reversed.query_family_identity,
        original.query_family_identity
    );
    assert_eq!(
        reversed.collection_cohort_identity,
        original.collection_cohort_identity
    );
}

#[test]
fn decisions_require_human_attestation_and_authoritative_source() {
    let mut decisions = ReviewDecisions {
        contract: DECISIONS_CONTRACT.to_owned(),
        schema_version: 1,
        reviewer_identity: "curator@example".to_owned(),
        reviewed_at_unix_seconds: 1,
        attestation: ReviewAttestation::HumanReviewed,
        authorization_context: None,
        decisions: vec![ReviewDecision {
            judgment_identity: "00".repeat(32),
            verdict: ReviewVerdict::PositivePreferred,
            reason: JudgmentReasonV3::PhraseOrderFailure,
            source: JudgmentSourceV3::CuratedRegressionCase,
            confidence: 1.0,
        }],
    };
    assert!(decisions.validate().is_ok());
    decisions.attestation = ReviewAttestation::AgentCuratedWithUserAuthorization;
    assert!(decisions.validate().is_err());
    decisions.authorization_context = Some("user-authorized-test".to_owned());
    assert!(decisions.validate().is_ok());
    decisions.decisions[0].source = JudgmentSourceV3::AutomaticallyMinedNegative;
    assert!(decisions.validate().is_err());
}

#[test]
fn application_supersedes_candidate_and_activates_only_review() {
    let directory = tempfile::tempdir().unwrap();
    let ledger_path = directory.path().join("candidate.json");
    let decisions_path = directory.path().join("decisions.json");
    let output_path = directory.path().join("reviewed.json");
    let receipt_path = directory.path().join("receipt.json");
    let positive = KeyedIdentity::from_bytes([20; 32]);
    let negative = KeyedIdentity::from_bytes([21; 32]);
    let evidence = RankEvidenceV3 {
        schema_version: 3,
        query_groups: 1,
        matched_groups: 1,
        missing_groups: 0,
        query_flags: 0,
        field_count: 1,
        values: [0.5; 30],
    };
    let candidate = PairwiseJudgmentV3::from_draft(PairwiseJudgmentDraftV3 {
        workspace_identity: KeyedIdentity::from_bytes([1; 32]),
        query_identity: KeyedIdentity::from_bytes([2; 32]),
        positive_document_version: positive,
        negative_document_version: negative,
        positive_features: evidence,
        negative_features: evidence,
        positive_tier: RelevanceTier::CompleteExactGroups,
        negative_tier: RelevanceTier::CompleteExactGroups,
        candidate_pool: vec![positive, negative].into_boxed_slice(),
        positive_position: 0,
        negative_position: 1,
        split_groups: groups(),
        frozen_holdout: None,
        v2_model_identity: [8; 32],
        challenger_model_identity: [9; 32],
        reason: JudgmentReasonV3::PhraseOrderFailure,
        source: JudgmentSourceV3::AutomaticallyMinedNegative,
        confidence: 0.5,
        weight: 0.5,
        index_generation: 1,
        supersedes: None,
        contradicts: Box::new([]),
    });
    let mut ledger = RelevanceLedgerV3::default();
    ledger.append(candidate.clone()).unwrap();
    std::fs::write(&ledger_path, serde_json::to_vec(&ledger).unwrap()).unwrap();
    std::fs::write(
        &decisions_path,
        serde_json::to_vec(&serde_json::json!({
            "contract": DECISIONS_CONTRACT,
            "schema_version": 1,
            "reviewer_identity": "test-fixture-curator",
            "reviewed_at_unix_seconds": 1,
            "attestation": "human_reviewed",
            "authorization_context": null,
            "decisions": [{
                "judgment_identity": hex(candidate.identity.as_bytes()),
                "verdict": "positive_preferred",
                "reason": "phrase_order_failure",
                "source": "curated_regression_case",
                "confidence": 1.0
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    let publication = apply(&ledger_path, &decisions_path, &output_path, &receipt_path).unwrap();
    assert_eq!(publication.appended, 1);
    assert_eq!(publication.active_training_judgments, 1);
    let reviewed: RelevanceLedgerV3 =
        serde_json::from_slice(&std::fs::read(&output_path).unwrap()).unwrap();
    assert_eq!(reviewed.judgments.len(), 2);
    assert_eq!(
        reviewed.active_model_training_judgments()[0].source,
        JudgmentSourceV3::CuratedRegressionCase
    );
    assert_eq!(
        reviewed.active_model_training_judgments()[0].supersedes,
        Some(candidate.identity)
    );

    let repeated_ledger = directory.path().join("reviewed-repeated.json");
    let repeated_receipt = directory.path().join("receipt-repeated.json");
    let repeated = apply(
        &output_path,
        &decisions_path,
        &repeated_ledger,
        &repeated_receipt,
    )
    .unwrap();
    assert_eq!(repeated.submitted, 1);
    assert_eq!(repeated.appended, 0);
    assert_eq!(repeated.already_applied, 1);
    assert_eq!(repeated.revised, 0);
    let unchanged: RelevanceLedgerV3 =
        serde_json::from_slice(&std::fs::read(&repeated_ledger).unwrap()).unwrap();
    assert_eq!(unchanged, reviewed);

    std::fs::write(
        &decisions_path,
        serde_json::to_vec(&serde_json::json!({
            "contract": DECISIONS_CONTRACT,
            "schema_version": 1,
            "reviewer_identity": "test-fixture-curator",
            "reviewed_at_unix_seconds": 2,
            "attestation": "human_reviewed",
            "authorization_context": null,
            "decisions": [{
                "judgment_identity": hex(candidate.identity.as_bytes()),
                "verdict": "negative_preferred",
                "reason": "phrase_order_failure",
                "source": "curated_regression_case",
                "confidence": 0.9
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    let revised_ledger = directory.path().join("reviewed-revised.json");
    let revised_receipt = directory.path().join("receipt-revised.json");
    let revision = apply(
        &repeated_ledger,
        &decisions_path,
        &revised_ledger,
        &revised_receipt,
    )
    .unwrap();
    assert_eq!(revision.appended, 1);
    assert_eq!(revision.already_applied, 0);
    assert_eq!(revision.revised, 1);
    let revised: RelevanceLedgerV3 =
        serde_json::from_slice(&std::fs::read(revised_ledger).unwrap()).unwrap();
    assert_eq!(revised.judgments.len(), 3);
    let active = revised.active_model_training_judgments();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].positive_document_version, negative);
    assert_eq!(active[0].negative_document_version, positive);
    assert_eq!(active[0].supersedes, Some(reviewed.judgments[1].identity));
    assert!(active[0]
        .contradicts
        .contains(&reviewed.judgments[1].identity));
}
