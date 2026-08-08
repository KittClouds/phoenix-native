use crate::*;
use phoenix_graph_generation_v2::{CandidateEvidenceBindingRecord, CandidateId, CandidateStatus};
use phoenix_memory_contract::{
    CandidateEndpointBindingRecordV3, CandidateEndpointRoleV3, ContentUnitKind, DocumentChunkInput,
    DocumentInput, EvidenceRecordV3, MixedSourceBuilder, SemanticCandidateFamilyV3,
    SemanticCandidateRecordV3, StringRef, VerifiedGraphGenerationV3, VocabularyPackKindV3,
    VocabularyPackRecordV3,
};
use phoenix_memory_semantics::{
    DeterministicAdjudicatorV1, MemoryEventV1, NliRelationV1, PolicyProposalV1, ScopeRelationV1,
    SemanticAdjudicationInputV1, SourceAuthorityV1, TemporalRelationV1, CUE_EXPLICIT_CORRECTION,
};
use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::time::{Duration, Instant};
use tempfile::TempDir;

const OLD_ID: [u8; 32] = [0x11; 32];
const NEW_ID: [u8; 32] = [0x22; 32];

#[test]
fn ledger_is_append_only_idempotent_and_restart_verified() {
    let fixture = Fixture::new();
    let ledger_root = fixture.temp.path().join("ledger");
    let mut ledger = PolicyDecisionLedgerV1::open(&ledger_root).expect("open ledger");
    let binding = fixture.catalog.get(OLD_ID).expect("old candidate");
    let command = command(binding, add_proposal(), [0; 32], 1_000, 0);
    let mut stale = command.clone();
    stale.expected_source_generation_hash = [0x99; 32];
    assert!(matches!(
        ledger.decide(&fixture.catalog, &stale),
        Err(MemoryRuntimeError::StaleCandidateBinding)
    ));
    assert!(ledger.receipts().is_empty());
    let first = ledger
        .decide(&fixture.catalog, &command)
        .expect("append decision");
    let repeated = ledger
        .decide(&fixture.catalog, &command)
        .expect("reuse decision");
    assert_eq!(first.receipt_id, repeated.receipt_id);
    assert_eq!(first.sequence, 1);
    assert!(!first.reused);
    assert!(repeated.reused);
    assert_eq!(ledger.receipts().len(), 1);
    drop(ledger);

    let reopened = PolicyDecisionLedgerV1::open(&ledger_root).expect("restart ledger");
    assert_eq!(reopened.sequence(), 1);
    assert_eq!(reopened.receipts()[0].reason(), "policy-qualified memory");
    assert_eq!(
        reopened.head(OLD_ID).unwrap().header().receipt_id,
        first.receipt_id
    );
}

#[test]
fn missing_receipt_breaks_the_global_sequence_chain() {
    let fixture = Fixture::new();
    let ledger_root = fixture.temp.path().join("missing-ledger");
    let mut ledger = PolicyDecisionLedgerV1::open(&ledger_root).expect("open ledger");
    for candidate_id in [OLD_ID, NEW_ID] {
        let decided_at = ledger.sequence() as i64 + 1;
        ledger
            .decide(
                &fixture.catalog,
                &command(
                    fixture.catalog.get(candidate_id).unwrap(),
                    add_proposal(),
                    [0; 32],
                    decided_at,
                    0,
                ),
            )
            .expect("append chained receipt");
    }
    let first_path = ledger.receipts()[0].path().to_path_buf();
    drop(ledger);
    std::fs::remove_file(first_path).expect("remove first receipt from isolated fixture");
    assert!(matches!(
        PolicyDecisionLedgerV1::open(&ledger_root),
        Err(MemoryRuntimeError::InvalidDecisionChain)
    ));
}

#[test]
fn replacement_closes_validity_and_undo_restores_prior_current_memory() {
    let fixture = Fixture::new();
    let mut ledger = PolicyDecisionLedgerV1::open(fixture.temp.path().join("replacement-ledger"))
        .expect("open ledger");
    let old = fixture.catalog.get(OLD_ID).unwrap();
    let new = fixture.catalog.get(NEW_ID).unwrap();
    ledger
        .decide(
            &fixture.catalog,
            &command(old, add_proposal(), [0; 32], 100, 0),
        )
        .expect("accept old memory");
    ledger
        .decide(
            &fixture.catalog,
            &command(new, supersede_proposal(), OLD_ID, 600, 500),
        )
        .expect("supersede old memory");

    let projection = CurrentMemoryProjectionV1::materialize(&fixture.catalog, &ledger)
        .expect("materialize replacement");
    let projection_path = fixture
        .temp
        .path()
        .join(format!("current.{}", CURRENT_MEMORY_PROJECTION_EXTENSION));
    let verified_projection = projection
        .publish(&projection_path)
        .expect("publish immutable projection");
    assert_eq!(
        verified_projection.header().projection_hash,
        projection.receipt().projection_hash
    );
    assert_eq!(
        verified_projection.records().len(),
        projection.records().len()
    );
    drop(verified_projection);
    let reopened_projection =
        VerifiedCurrentMemoryProjectionV1::open(&projection_path).expect("reopen projection");
    assert_eq!(
        reopened_projection.header().projection_hash,
        projection.receipt().projection_hash
    );
    let old_record = projection.get(OLD_ID).unwrap();
    let new_record = projection.get(NEW_ID).unwrap();
    assert_eq!(old_record.memory_state(), CurrentMemoryStateV1::Superseded);
    assert_eq!(old_record.valid_time_to_millis, 500);
    assert_eq!(old_record.system_sequence_to, 2);
    assert_eq!(new_record.memory_state(), CurrentMemoryStateV1::Active);
    assert_eq!(new_record.valid_time_from_millis, 500);
    assert_eq!(
        projection
            .active_at(400, 1)
            .map(|record| record.candidate_id)
            .collect::<Vec<_>>(),
        [OLD_ID]
    );
    assert_eq!(
        projection
            .active_at(600, 1)
            .map(|record| record.candidate_id)
            .collect::<Vec<_>>(),
        [OLD_ID]
    );
    assert_eq!(
        projection
            .active_at(600, 2)
            .map(|record| record.candidate_id)
            .collect::<Vec<_>>(),
        [NEW_ID]
    );

    let mut undo = command(new, supersede_proposal(), [0; 32], 700, 500);
    undo.disposition = DecisionDispositionV1::Undo;
    ledger
        .decide(&fixture.catalog, &undo)
        .expect("undo replacement");
    let restored = CurrentMemoryProjectionV1::materialize(&fixture.catalog, &ledger)
        .expect("materialize undo");
    assert_eq!(restored.records().len(), 1);
    assert_eq!(restored.records()[0].candidate_id, OLD_ID);
    assert_eq!(
        restored.records()[0].memory_state(),
        CurrentMemoryStateV1::Active
    );
}

#[test]
fn recursive_working_set_is_stable_bounded_and_allocation_free_when_warm() {
    let fixture = Fixture::new();
    let mut ledger = PolicyDecisionLedgerV1::open(fixture.temp.path().join("graph-ledger"))
        .expect("open ledger");
    for (sequence, candidate_id) in [OLD_ID, NEW_ID].into_iter().enumerate() {
        let binding = fixture.catalog.get(candidate_id).unwrap();
        ledger
            .decide(
                &fixture.catalog,
                &command(binding, add_proposal(), [0; 32], sequence as i64 + 1, 0),
            )
            .expect("commit graph memory");
    }
    let projection = CurrentMemoryProjectionV1::materialize(&fixture.catalog, &ledger)
        .expect("materialize graph projection");
    let graph = WorkingSetGraphV1::build(&fixture.generation, &projection, 600, ledger.sequence())
        .expect("build packed graph");
    let seeds = [WorkingNodeKeyV1::Candidate(OLD_ID)];
    let query = RecursiveQueryV1 {
        seeds: &seeds,
        max_depth: 2,
        max_nodes: 32,
        max_edges: 128,
    };
    let mut scratch = RecursiveScratchV1::new(graph.nodes().len(), 32).expect("scratch");
    let cold = graph.traverse(query, &mut scratch).expect("cold traversal");
    let first = scratch.visited().to_vec();
    let warm = graph.traverse(query, &mut scratch).expect("warm traversal");
    assert_eq!(first, scratch.visited());
    assert_eq!(cold.visited_nodes, 4);
    assert!(scratch
        .visited()
        .iter()
        .filter_map(|id| graph.node(*id))
        .any(|node| node.candidate_id == NEW_ID));
    assert!(!warm.allocations_grew);
    assert!(!warm.truncated);

    let start = Instant::now();
    for _ in 0..10_000 {
        let receipt = graph
            .traverse(query, &mut scratch)
            .expect("performance traversal");
        assert!(!receipt.allocations_grew);
    }
    assert!(start.elapsed() < Duration::from_secs(1));

    let forward_seeds = [
        WorkingNodeKeyV1::Candidate(OLD_ID),
        WorkingNodeKeyV1::Candidate(NEW_ID),
    ];
    let reverse_seeds = [forward_seeds[1], forward_seeds[0]];
    let ordered_query = RecursiveQueryV1 {
        seeds: &forward_seeds,
        ..query
    };
    graph
        .traverse(ordered_query, &mut scratch)
        .expect("forward seeds");
    let forward = scratch.visited().to_vec();
    graph
        .traverse(
            RecursiveQueryV1 {
                seeds: &reverse_seeds,
                ..query
            },
            &mut scratch,
        )
        .expect("reverse seeds");
    assert_eq!(forward, scratch.visited());

    let bounded = graph
        .traverse(
            RecursiveQueryV1 {
                max_nodes: 2,
                ..query
            },
            &mut scratch,
        )
        .expect("bounded traversal");
    assert!(bounded.truncated);
    assert_eq!(bounded.visited_nodes, 2);
}

#[test]
fn corrupt_receipt_breaks_restart_verification() {
    let fixture = Fixture::new();
    let ledger_root = fixture.temp.path().join("corrupt-ledger");
    let mut ledger = PolicyDecisionLedgerV1::open(&ledger_root).expect("open ledger");
    let binding = fixture.catalog.get(OLD_ID).unwrap();
    ledger
        .decide(
            &fixture.catalog,
            &command(binding, add_proposal(), [0; 32], 1, 0),
        )
        .expect("decision");
    let path = ledger.receipts()[0].path().to_path_buf();
    drop(ledger);
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    file.seek(SeekFrom::End(-1)).unwrap();
    let mut byte = [0_u8; 1];
    file.read_exact(&mut byte).unwrap();
    file.seek(SeekFrom::End(-1)).unwrap();
    file.write_all(&[byte[0] ^ 0xff]).unwrap();
    file.sync_all().unwrap();
    drop(file);
    assert!(matches!(
        PolicyDecisionLedgerV1::open(&ledger_root),
        Err(MemoryRuntimeError::CorruptDecision(_))
    ));
}

struct Fixture {
    temp: TempDir,
    generation: VerifiedGraphGenerationV3,
    catalog: MemoryCatalogV1,
}

impl Fixture {
    fn new() -> Self {
        let temp = TempDir::new().expect("temporary runtime fixture");
        let path = temp.path().join("fixture.phxgg3");
        let generation = generation(&path);
        let catalog = MemoryCatalogV1::from_generation(&generation).expect("catalog");
        Self {
            temp,
            generation,
            catalog,
        }
    }
}

fn generation(path: &Path) -> VerifiedGraphGenerationV3 {
    let mut prepared = MixedSourceBuilder::new(b"memory-runtime-test")
        .generations(1, 1, 1)
        .add_document(DocumentInput::current(
            b"note/runtime".to_vec(),
            1,
            "Phoenix / Runtime",
            "Alice likes tea.",
            vec![DocumentChunkInput {
                start: 0,
                end: 16,
                sentence_start: 0,
                sentence_end: 1,
                paragraph_start: 0,
                paragraph_end: 1,
                chapter_index: 0,
                token_count: 3,
                flags: 1,
            }],
            1,
        ))
        .prepare()
        .expect("prepare source generation");
    let source_id = prepared.pages.sources[0].id;
    let unit_id = prepared
        .pages
        .content_units
        .iter()
        .find(|unit| unit.kind == ContentUnitKind::Document as u16)
        .expect("document unit")
        .id;
    let pack_name = append_string(&mut prepared.pages.strings, "phoenix.runtime");
    let pack_version = append_string(&mut prepared.pages.strings, "1");
    prepared
        .pages
        .vocabulary_packs
        .push(VocabularyPackRecordV3 {
            id: 77,
            name: pack_name,
            version: pack_version,
            schema_hash: [0x31; 32],
            producer_identity_hash: [0x32; 32],
            kind: VocabularyPackKindV3::Core as u16,
            flags: 0,
            reserved: 0,
        });
    for (ordinal, (candidate_id, value, evidence_id, object_id, valid_from)) in [
        (OLD_ID, "tea", 501_u64, 200_u64, 0_i64),
        (NEW_ID, "coffee", 502_u64, 300_u64, 500_i64),
    ]
    .into_iter()
    .enumerate()
    {
        prepared.pages.evidence.push(EvidenceRecordV3 {
            id: evidence_id,
            source_id,
            entity_id: 0,
            mention_id: 0,
            content_unit_id: unit_id,
            start: 0,
            end: 5,
            role: phoenix_graph_generation_v2::EvidenceRole::Premise as u16,
            flags: 0,
            reserved: 0,
        });
        let relation = append_string(&mut prepared.pages.strings, "core.preference");
        let value = append_string(&mut prepared.pages.strings, value);
        prepared
            .pages
            .semantic_candidates
            .push(SemanticCandidateRecordV3 {
                candidate_id,
                source_id,
                vocabulary_pack_id: 77,
                relation_kind: relation,
                value,
                endpoint_start: 0,
                endpoint_count: 2,
                evidence_start: 0,
                evidence_count: 1,
                valid_time_from_millis: valid_from,
                valid_time_to_millis: i64::MAX,
                system_generation_from: 1,
                system_generation_to: u64::MAX,
                confidence_bits: 0.95_f32.to_bits(),
                model_identity_index: u32::MAX,
                producer_identity_hash: [0x32; 32],
                family: SemanticCandidateFamilyV3::Attribute as u16,
                status: CandidateStatus::Proposed as u16,
                flags: 0,
                reserved: [0; 2],
            });
        for (endpoint_ordinal, entity_id) in [100_u64, object_id].into_iter().enumerate() {
            prepared
                .pages
                .candidate_endpoint_bindings
                .push(CandidateEndpointBindingRecordV3 {
                    candidate_id,
                    endpoint_id: entity_id,
                    ordinal: endpoint_ordinal as u32,
                    role: if endpoint_ordinal == 0 {
                        CandidateEndpointRoleV3::Subject as u16
                    } else {
                        CandidateEndpointRoleV3::Object as u16
                    },
                    flags: 0,
                });
        }
        prepared
            .pages
            .candidate_evidence_bindings
            .push(CandidateEvidenceBindingRecord {
                candidate_id: CandidateId(candidate_id),
                evidence_id,
                ordinal: 0,
                role: phoenix_graph_generation_v2::EvidenceRole::Premise as u16,
                flags: ordinal as u16,
            });
    }
    prepared.write(path).expect("write verified fixture")
}

fn append_string(strings: &mut Vec<u8>, value: &str) -> StringRef {
    let reference = StringRef {
        offset: strings.len() as u64,
        length: value.len() as u32,
        reserved: 0,
    };
    strings.extend_from_slice(value.as_bytes());
    reference
}

fn command(
    binding: &CandidateBindingV1,
    proposal: PolicyProposalV1,
    replacement_target_id: [u8; 32],
    decided_at: i64,
    effective_at: i64,
) -> PolicyDecisionCommandV1 {
    PolicyDecisionCommandV1 {
        candidate_id: binding.candidate_id,
        expected_source_generation_hash: binding.source_generation_hash,
        expected_candidate_hash: binding.candidate_hash,
        expected_evidence_hash: binding.evidence_hash,
        replacement_target_id,
        policy_identity: DeterministicAdjudicatorV1::default().policy_identity(),
        semantic_observation_hash: [0x73; 32],
        source_authority: SourceAuthorityV1::SubjectExplicit,
        proposal,
        disposition: DecisionDispositionV1::Commit,
        decided_at_unix_millis: decided_at,
        effective_at_unix_millis: effective_at,
        reason: "policy-qualified memory".to_owned(),
    }
}

fn add_proposal() -> PolicyProposalV1 {
    adjudicate(
        MemoryEventV1::PreferenceShift,
        NliRelationV1::Neutral,
        ScopeRelationV1::Different,
        TemporalRelationV1::CurrentOverCurrent,
        0,
    )
}

fn supersede_proposal() -> PolicyProposalV1 {
    adjudicate(
        MemoryEventV1::ExplicitCorrection,
        NliRelationV1::Contradiction,
        ScopeRelationV1::Same,
        TemporalRelationV1::CurrentOverCurrent,
        CUE_EXPLICIT_CORRECTION,
    )
}

fn adjudicate(
    memory_event: MemoryEventV1,
    nli_relation: NliRelationV1,
    scope_relation: ScopeRelationV1,
    temporal_relation: TemporalRelationV1,
    semantic_cues: u32,
) -> PolicyProposalV1 {
    DeterministicAdjudicatorV1::default()
        .adjudicate(SemanticAdjudicationInputV1 {
            memory_event,
            memory_event_score: 0.95,
            nli_relation,
            nli_score: 0.95,
            scope_relation,
            temporal_relation,
            source_authority: SourceAuthorityV1::SubjectExplicit,
            semantic_cues,
            evidence_count: 1,
            gliclass_role: phoenix_memory_contract::ModelSemanticRoleV3::SteerableSemanticObserver,
            modernbert_role: phoenix_memory_contract::ModelSemanticRoleV3::DedicatedNliObserver,
        })
        .expect("constitutional adjudication")
}
