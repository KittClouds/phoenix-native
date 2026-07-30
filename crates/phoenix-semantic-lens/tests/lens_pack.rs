use phoenix_graph_generation_v2::CandidateId;
use phoenix_semantic_lens::{
    validate_definition, validate_review_binding, validate_review_binding_against_pack,
    write_semantic_lens_pack_new, CandidateKeyBuilder, CandidateOrigin, CoreSemanticClass,
    EndpointKind, EndpointMask, LensCodeDefinition, LensNeutralReviewBinding, SemanticEndpointRef,
    SemanticLensDefinition, SemanticLensError, VerifiedSemanticLensPackV1,
};
use std::{
    fs::OpenOptions,
    io::{Seek, SeekFrom, Write},
    mem::size_of,
};
use tempfile::tempdir;

const RELATION: u32 = ((CoreSemanticClass::Relation as u32) << 16) | 1;
const OCCURRENCE: u32 = ((CoreSemanticClass::Occurrence as u32) << 16) | 1;
const GROUPING: u32 = ((CoreSemanticClass::Grouping as u32) << 16) | 1;
const STATE: u32 = ((CoreSemanticClass::AttributedState as u32) << 16) | 1;

const RESEARCH_CODES: [LensCodeDefinition<'static>; 4] = [
    LensCodeDefinition {
        code: RELATION,
        stable_name: "relation.supports",
        class: CoreSemanticClass::Relation,
        source_endpoints: EndpointMask::ENTITY,
        target_endpoints: EndpointMask::ENTITY,
        flags: 0,
    },
    LensCodeDefinition {
        code: OCCURRENCE,
        stable_name: "occurrence.experiment",
        class: CoreSemanticClass::Occurrence,
        source_endpoints: EndpointMask::CHUNK,
        target_endpoints: EndpointMask::NONE,
        flags: 0,
    },
    LensCodeDefinition {
        code: GROUPING,
        stable_name: "grouping.study",
        class: CoreSemanticClass::Grouping,
        source_endpoints: EndpointMask::GROUPING,
        target_endpoints: EndpointMask::CHUNK.union(EndpointMask::OCCURRENCE),
        flags: 0,
    },
    LensCodeDefinition {
        code: STATE,
        stable_name: "state.result",
        class: CoreSemanticClass::AttributedState,
        source_endpoints: EndpointMask::ENTITY,
        target_endpoints: EndpointMask::OCCURRENCE,
        flags: 0,
    },
];

fn research_definition() -> SemanticLensDefinition<'static> {
    SemanticLensDefinition {
        namespace: "phoenix.research/v0",
        version: 1,
        configuration_hash: *blake3::hash(b"research-witness-config").as_bytes(),
        codes: &RESEARCH_CODES,
    }
}

#[test]
fn packed_layout_is_compact_and_frozen() {
    assert_eq!(size_of::<phoenix_semantic_lens::SemanticLensHeader>(), 320);
    assert_eq!(size_of::<phoenix_semantic_lens::SemanticCodeRecord>(), 32);
    assert_eq!(size_of::<CandidateOrigin>(), 108);
    assert_eq!(size_of::<LensNeutralReviewBinding>(), 264);
}

#[test]
fn research_witness_opens_as_a_hash_bound_compact_pack() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("research.pslp");
    let generation_hash = *blake3::hash(b"generation").as_bytes();
    let pack =
        write_semantic_lens_pack_new(&path, &research_definition(), generation_hash).unwrap();

    assert_eq!(pack.header().bound_generation_hash, generation_hash);
    assert_eq!(pack.codes().len(), RESEARCH_CODES.len());
    assert_eq!(
        pack.code_name(pack.code(RELATION).unwrap()).unwrap(),
        "relation.supports"
    );
    assert!(matches!(
        CandidateOrigin::from_pack(&pack, 999, CandidateId([7; 32])),
        Err(SemanticLensError::UnknownCode(999))
    ));
}

#[test]
fn lens_namespace_isolates_identical_candidate_material() {
    let content_hash = *blake3::hash(b"same source").as_bytes();
    let mut story = CandidateKeyBuilder::producer_scoped(
        "phoenix.story-candidate/v1",
        b"relationship",
        &content_hash,
        "deterministic/relation-v1",
    )
    .unwrap();
    let mut research = CandidateKeyBuilder::producer_scoped(
        "phoenix.research-candidate/v0",
        b"relationship",
        &content_hash,
        "deterministic/relation-v1",
    )
    .unwrap();
    for builder in [&mut story, &mut research] {
        builder
            .update_u64(11)
            .update_u64(12)
            .update_u16(1)
            .update_u64(101)
            .update_u64(102);
    }
    assert_ne!(story.finish(), research.finish());
}

#[test]
fn review_binding_is_lens_neutral_and_evidence_bound() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("research-review.pslp");
    let pack = write_semantic_lens_pack_new(
        &path,
        &research_definition(),
        *blake3::hash(b"generation").as_bytes(),
    )
    .unwrap();
    let candidate_id = CandidateId(*blake3::hash(b"candidate").as_bytes());
    let binding = LensNeutralReviewBinding {
        origin: CandidateOrigin::from_pack(&pack, RELATION, candidate_id).unwrap(),
        origin_alignment_padding: 0,
        source: SemanticEndpointRef::new(EndpointKind::Entity, 11),
        target: SemanticEndpointRef::new(EndpointKind::Entity, 12),
        document_hash: *blake3::hash(b"document").as_bytes(),
        candidate_hash: *blake3::hash(b"candidate-row").as_bytes(),
        evidence_hash: *blake3::hash(b"evidence").as_bytes(),
        producer_generation: 9,
        registry_revision: 3,
        flags: 0,
        reserved: 0,
    };
    validate_review_binding(&binding).unwrap();
    validate_review_binding_against_pack(&binding, &pack).unwrap();

    let mut invalid = binding;
    invalid.evidence_hash = [0; 32];
    assert!(matches!(
        validate_review_binding(&invalid),
        Err(SemanticLensError::InvalidReviewBinding)
    ));

    let mut wrong_endpoint = binding;
    wrong_endpoint.target = SemanticEndpointRef::new(EndpointKind::Occurrence, 12);
    assert!(matches!(
        validate_review_binding_against_pack(&wrong_endpoint, &pack),
        Err(SemanticLensError::InvalidReviewBinding)
    ));
}

#[test]
fn corrupt_pack_and_duplicate_vocabulary_fail_closed() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("corrupt.pslp");
    write_semantic_lens_pack_new(
        &path,
        &research_definition(),
        *blake3::hash(b"generation").as_bytes(),
    )
    .unwrap();

    let mut file = OpenOptions::new().write(true).open(&path).unwrap();
    file.seek(SeekFrom::End(-1)).unwrap();
    file.write_all(&[0xff]).unwrap();
    file.sync_all().unwrap();
    assert!(matches!(
        VerifiedSemanticLensPackV1::open(&path),
        Err(SemanticLensError::PayloadHashMismatch)
            | Err(SemanticLensError::InvalidStringReference)
    ));

    let duplicate = [
        RESEARCH_CODES[0],
        LensCodeDefinition {
            stable_name: "relation.duplicate",
            ..RESEARCH_CODES[0]
        },
    ];
    let definition = SemanticLensDefinition {
        codes: &duplicate,
        ..research_definition()
    };
    assert!(matches!(
        validate_definition(&definition),
        Err(SemanticLensError::DuplicateCode(RELATION))
    ));
}
