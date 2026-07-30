use crate::*;
use bytemuck::{bytes_of, cast_slice, Zeroable};
use phoenix_graph_generation::{
    SectionKind as V1SectionKind, GRAPH_GENERATION_CONTRACT as V1_CONTRACT,
    GRAPH_GENERATION_EXTENSION as V1_EXTENSION, GRAPH_GENERATION_VERSION as V1_VERSION,
};
use std::fs;
use std::mem::{align_of, size_of};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn v1_public_contract_remains_frozen() {
    assert_eq!(V1_CONTRACT, "phoenix.graph-generation/v1");
    assert_eq!(V1_VERSION, 1);
    assert_eq!(V1_EXTENSION, "phxgg");
    assert_eq!(V1SectionKind::ALL.len(), 15);
    assert_eq!(
        V1SectionKind::ALL.map(|kind| kind as u16),
        [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]
    );
}

#[test]
fn v2_tags_and_layout_are_frozen() {
    assert_eq!(GRAPH_GENERATION_V2_CONTRACT, "phoenix.graph-generation/v2");
    assert_eq!(GRAPH_GENERATION_V2_MAGIC, *b"PHXGG002");
    assert_eq!(GRAPH_GENERATION_V2_VERSION, 2);
    assert_eq!(PageKind::ALL.len(), 28);
    assert_eq!(
        PageKind::ALL.map(|kind| kind as u16),
        [
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
            25, 26, 27, 28,
        ]
    );
    assert_eq!(size_of::<GenerationHeader>(), 256);
    assert_eq!(align_of::<GenerationHeader>(), 8);
    assert_eq!(size_of::<PageDescriptor>(), 128);
    assert_eq!(align_of::<PageDescriptor>(), 8);
    assert_eq!(
        PageKind::ALL.map(expected_record_size),
        [
            1, 80, 64, 56, 64, 64, 80, 56, 56, 48, 40, 80, 72, 56, 56, 40, 72, 72, 104, 56, 56,
            120, 48, 120, 80, 112, 48, 32,
        ]
    );
    assert_eq!(
        PageKind::ALL.map(expected_record_alignment),
        [1, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 4, 8, 8, 8, 8, 8, 8, 8,]
    );
}

#[test]
fn every_expected_family_has_a_distinct_page() {
    let semantic_pages = [
        PageKind::TypedRelationshipCandidates,
        PageKind::IdentityCandidates,
        PageKind::CandidateEvidenceBindings,
        PageKind::Events,
        PageKind::Episodes,
        PageKind::EpisodeMemberships,
        PageKind::TemporalCandidates,
        PageKind::CausalCandidates,
        PageKind::MemoryStateCandidates,
        PageKind::ContextualEvidence,
        PageKind::NliAdjudications,
        PageKind::Decisions,
    ];
    for page in semantic_pages {
        assert!(PageKind::ALL.contains(&page));
        assert!(expected_record_size(page) > 0);
    }
}

#[test]
fn page_authority_is_unambiguous() {
    for kind in PageKind::ALL {
        let authority = expected_authority(kind);
        match kind {
            PageKind::Strings
            | PageKind::Documents
            | PageKind::Chapters
            | PageKind::Paragraphs
            | PageKind::Sentences
            | PageKind::Chunks
            | PageKind::Spans
            | PageKind::Entities
            | PageKind::Mentions
            | PageKind::Evidence
            | PageKind::CanonicalEntityBindings
            | PageKind::StructuralEdges => {
                assert_eq!(authority, AuthorityClass::SourceAuthoritative);
            }
            PageKind::ContextualEvidence => {
                assert_eq!(authority, AuthorityClass::ContextualEvidenceOnly);
            }
            PageKind::Decisions => {
                assert_eq!(authority, AuthorityClass::DecisionReceipt);
            }
            PageKind::Capabilities
            | PageKind::ModelIdentities
            | PageKind::StageReceipts
            | PageKind::PublicationReceipts => {
                assert_eq!(authority, AuthorityClass::RuntimeReceipt);
            }
            _ => assert_eq!(authority, AuthorityClass::SemanticCandidate),
        }
        assert_ne!(authority, AuthorityClass::ProjectionOnly);
    }
}

#[test]
fn schema_directory_hash_is_frozen() {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"phoenix.graph-generation/v2/schema-directory\0");
    for kind in PageKind::ALL {
        hasher.update(&expected_schema_hash(kind));
    }
    assert_eq!(
        hasher.finalize().to_hex().as_str(),
        "3d0486e6e33145fcebbca97186edd2c55d6d5ae76f0e2213106933414e54a66a"
    );
}

#[test]
fn mmap_open_verifies_all_pages_once() {
    let path = write_fixture("valid", fixture_bytes());
    let opened = VerifiedGraphGenerationV2::open(&path).expect("open valid V2 fixture");
    assert_eq!(opened.directory().len(), PageKind::ALL.len());
    let resolved = opened
        .resolve_string(StringRef {
            offset: 0,
            length: 6,
            reserved: 0,
        })
        .expect("resolve string");
    assert_eq!(resolved, "oracle");
    let episodes = opened
        .typed_page::<EpisodeRecord>(PageKind::Episodes)
        .expect("view episode page");
    assert!(episodes.is_empty());
    fs::remove_file(path).expect("remove V2 fixture");
}

#[test]
fn corrupt_page_fails_closed() {
    let mut bytes = fixture_bytes();
    let header = *bytemuck::from_bytes::<GenerationHeader>(&bytes[..size_of::<GenerationHeader>()]);
    let directory_start = header.directory_offset as usize;
    let directory_end = directory_start + header.directory_len as usize;
    let directory =
        bytemuck::cast_slice::<u8, PageDescriptor>(&bytes[directory_start..directory_end]);
    let strings_offset = directory[0].offset as usize;
    bytes[strings_offset] ^= 0xff;
    let path = write_fixture("corrupt-page", bytes);
    assert!(matches!(
        VerifiedGraphGenerationV2::open(&path),
        Err(GraphGenerationV2Error::PageHashMismatch {
            page: PageKind::Strings
        })
    ));
    fs::remove_file(path).expect("remove corrupt fixture");
}

#[test]
fn wrong_authority_fails_before_publication() {
    let bytes = fixture_bytes();
    let header = *bytemuck::from_bytes::<GenerationHeader>(&bytes[..size_of::<GenerationHeader>()]);
    let directory_start = header.directory_offset as usize;
    let directory_end = directory_start + header.directory_len as usize;
    let mut directory =
        bytemuck::cast_slice::<u8, PageDescriptor>(&bytes[directory_start..directory_end]).to_vec();
    directory[PageKind::Episodes as usize - 1].authority =
        AuthorityClass::SourceAuthoritative as u16;
    let rebuilt = rebuild_fixture(header, directory, &bytes);
    let path = write_fixture("wrong-authority", rebuilt);
    assert!(matches!(
        VerifiedGraphGenerationV2::open(&path),
        Err(GraphGenerationV2Error::WrongAuthority {
            page: PageKind::Episodes,
            ..
        })
    ));
    fs::remove_file(path).expect("remove wrong-authority fixture");
}

#[test]
fn candidate_and_enum_tags_are_stable() {
    assert!(CandidateId::ZERO.is_zero());
    assert!(!CandidateId([1; 32]).is_zero());
    assert_eq!(CandidateStatus::Superseded as u16, 5);
    assert_eq!(DecisionAction::Undo as u16, 4);
    assert_eq!(CapabilityState::DurableVerified as u16, 2);
    assert_eq!(CacheState::DurableVerified as u16, 2);
    assert_eq!(ProducerProduct::DocumentStructure as u16, 1);
    assert_eq!(ProducerProduct::NliAdjudication as u16, 11);
    assert_eq!(SemanticFamily::ContextualCoOccurrence as u16, 10);
    assert_eq!(AuthorityClass::ProjectionOnly as u16, 5);
    assert_eq!(CanonicalBindingKind::CoordinatorDecision as u16, 2);
}

fn fixture_bytes() -> Vec<u8> {
    let header_size = size_of::<GenerationHeader>() as u64;
    let directory_offset = align_up(header_size, PAGE_ALIGNMENT);
    let directory_len = (PageKind::ALL.len() * size_of::<PageDescriptor>()) as u64;
    let mut cursor = align_up(directory_offset + directory_len, PAGE_ALIGNMENT);
    let strings = b"oracle";
    let mut directory = Vec::with_capacity(PageKind::ALL.len());

    for kind in PageKind::ALL {
        cursor = align_up(cursor, PAGE_ALIGNMENT);
        let payload = if kind == PageKind::Strings {
            strings.as_slice()
        } else {
            &[]
        };
        directory.push(PageDescriptor {
            kind: kind as u16,
            authority: expected_authority(kind) as u16,
            record_size: expected_record_size(kind),
            record_alignment: expected_record_alignment(kind),
            flags: PAGE_FLAG_REQUIRED,
            offset: cursor,
            length: payload.len() as u64,
            count: if kind == PageKind::Strings {
                payload.len() as u64
            } else {
                0
            },
            hash: *blake3::hash(payload).as_bytes(),
            schema_hash: expected_schema_hash(kind),
            reserved: [0; 3],
        });
        cursor += payload.len() as u64;
    }
    let total_len = align_up(cursor, PAGE_ALIGNMENT);
    let mut header = GenerationHeader::zeroed();
    header.magic = GRAPH_GENERATION_V2_MAGIC;
    header.version = GRAPH_GENERATION_V2_VERSION;
    header.header_size = header_size as u32;
    header.page_count = PageKind::ALL.len() as u32;
    header.flags = HEADER_FLAG_COMPLETE;
    header.total_len = total_len;
    header.directory_offset = directory_offset;
    header.directory_len = directory_len;
    header.source_document_id_hash = [1; 32];
    header.content_hash = [2; 32];
    header.cohort_hash = [3; 32];
    header.native_document_id = 7;
    header.document_revision = 11;
    header.registry_revision = 13;
    header.producer_generation = 17;
    header.published_generation = 19;
    header.generation_hash = compute_generation_hash(&header, &directory);

    let mut bytes = vec![0; total_len as usize];
    bytes[..header_size as usize].copy_from_slice(bytes_of(&header));
    let directory_end = directory_offset as usize + directory_len as usize;
    bytes[directory_offset as usize..directory_end].copy_from_slice(cast_slice(&directory));
    let strings_offset = directory[0].offset as usize;
    bytes[strings_offset..strings_offset + strings.len()].copy_from_slice(strings);
    bytes
}

fn rebuild_fixture(
    mut header: GenerationHeader,
    directory: Vec<PageDescriptor>,
    original: &[u8],
) -> Vec<u8> {
    header.generation_hash = compute_generation_hash(&header, &directory);
    let mut bytes = original.to_vec();
    bytes[..size_of::<GenerationHeader>()].copy_from_slice(bytes_of(&header));
    let directory_start = header.directory_offset as usize;
    let directory_end = directory_start + header.directory_len as usize;
    bytes[directory_start..directory_end].copy_from_slice(cast_slice(&directory));
    bytes
}

fn write_fixture(label: &str, bytes: Vec<u8>) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time")
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "phoenix-graph-generation-v2-{label}-{}-{nonce}.phxgg2",
        std::process::id()
    ));
    fs::write(&path, bytes).expect("write V2 fixture");
    path
}
