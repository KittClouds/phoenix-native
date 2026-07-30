use phoenix_analysis_contract::{
    AnalysisChunkRecord, AnalysisModelIdentity, AnalysisSentenceRecord, AnalysisSpanRecord,
    DocumentAnalysisBinding, PhoenixStructuralSubstrateV1, StructuralDialogueHint,
    StructuralSentenceQuality, StructuralSpanKind, NO_STRUCTURAL_PARENT,
    STRUCTURAL_SUBSTRATE_CONTRACT,
};
use phoenix_document_producer::{
    publish_or_reuse_structural_generation, DocumentProducerError, StructuralProducerInput,
    StructuralReuseState,
};
use phoenix_graph_generation_v2::{CapabilityState, PageKind};
use phoenix_scene_compiler::VerifiedStructuralSource;
use std::process::Command;

const PROBE_DIRECTORY: &str = "PHOENIX_STRUCTURAL_PROBE_DIRECTORY";

#[test]
fn same_source_produces_identical_ids_and_structural_page_hashes() {
    let fixture = fixture();
    let left_directory = tempfile::tempdir().unwrap();
    let right_directory = tempfile::tempdir().unwrap();
    let left = publish(left_directory.path(), &fixture).unwrap();
    let right = publish(right_directory.path(), &fixture).unwrap();

    assert_eq!(left.receipt().reuse_state, StructuralReuseState::Produced);
    assert_eq!(
        left.receipt().generation_hash,
        right.receipt().generation_hash
    );
    assert_eq!(left.receipt().pages, right.receipt().pages);
    assert_eq!(
        left.receipt().source_coordinate_hash,
        right.receipt().source_coordinate_hash
    );

    for kind in [
        PageKind::Documents,
        PageKind::Chapters,
        PageKind::Paragraphs,
        PageKind::Sentences,
        PageKind::Chunks,
        PageKind::Spans,
        PageKind::StructuralEdges,
    ] {
        assert_eq!(
            left.generation().page_bytes(kind),
            right.generation().page_bytes(kind),
            "{kind:?} drifted for identical authority"
        );
    }
}

#[test]
fn exact_dynamic_chunk_records_reach_the_verified_downstream_view() {
    let fixture = fixture();
    let directory = tempfile::tempdir().unwrap();
    let published = publish(directory.path(), &fixture).unwrap();
    let compiler_source = VerifiedStructuralSource::open(published.generation()).unwrap();
    let chunks = compiler_source.chunks();

    assert_eq!(chunks.len(), fixture.structural.chunks.len());
    for (packed, source) in chunks.iter().zip(&fixture.structural.chunks) {
        assert_eq!(packed.content_hash, source.content_hash);
        assert_eq!((packed.start, packed.end), (source.start, source.end));
        assert_eq!(
            (packed.sentence_start, packed.sentence_end),
            (source.sentence_start, source.sentence_end)
        );
        assert_eq!(
            (packed.paragraph_start, packed.paragraph_end),
            (source.paragraph_start, source.paragraph_end)
        );
        assert_eq!(packed.chapter_index, source.chapter_index);
        assert_eq!(packed.token_count, source.token_count);
    }

    assert_eq!(compiler_source.chapters().len(), 2);
    assert_eq!(compiler_source.paragraphs().len(), 2);
    assert_eq!(compiler_source.sentences().len(), 3);
    assert_eq!(compiler_source.spans().len(), 4);
    assert_eq!(compiler_source.document().chunk_count, 2);
    assert_eq!(compiler_source.structural_edges().len(), 9);
}

#[test]
fn fresh_process_durable_reuse_reports_durable_verified() {
    let fixture = fixture();
    let directory = tempfile::tempdir().unwrap();
    let first = publish(directory.path(), &fixture).unwrap();
    assert_eq!(first.receipt().reuse_state, StructuralReuseState::Produced);
    drop(first);

    let status = Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg("fresh_process_durable_verified_probe")
        .arg("--nocapture")
        .env(PROBE_DIRECTORY, directory.path())
        .status()
        .unwrap();
    assert!(status.success(), "fresh-process reuse probe failed");
}

#[test]
fn fresh_process_durable_verified_probe() {
    let Some(directory) = std::env::var_os(PROBE_DIRECTORY) else {
        return;
    };
    let fixture = fixture();
    let reopened = publish(std::path::Path::new(&directory), &fixture).unwrap();
    assert_eq!(
        reopened.receipt().reuse_state,
        StructuralReuseState::DurableVerified
    );
    assert_eq!(
        reopened.receipt().capability_state,
        CapabilityState::DurableVerified
    );
    assert_eq!(
        reopened.chunks().unwrap().len(),
        fixture.structural.chunks.len()
    );
}

#[test]
fn changed_source_cannot_reuse_or_publish_a_stale_structural_binding() {
    let fixture = fixture();
    let directory = tempfile::tempdir().unwrap();
    let error = publish_or_reuse_structural_generation(
        directory.path(),
        StructuralProducerInput {
            text: "changed",
            structural: &fixture.structural,
        },
    )
    .err()
    .expect("stale binding must fail");
    assert!(matches!(
        error,
        DocumentProducerError::SourceBindingMismatch
    ));
}

struct Fixture {
    text: String,
    structural: PhoenixStructuralSubstrateV1,
}

fn publish(
    directory: &std::path::Path,
    fixture: &Fixture,
) -> Result<phoenix_document_producer::VerifiedStructuralGeneration, DocumentProducerError> {
    publish_or_reuse_structural_generation(
        directory,
        StructuralProducerInput {
            text: &fixture.text,
            structural: &fixture.structural,
        },
    )
}

fn fixture() -> Fixture {
    let text = "## Chapter 1: Dawn\nAlpha arrived. Beta waited.\n## Chapter 2: Dusk\nGamma left."
        .to_owned();
    let chapter_two = text.find("## Chapter 2").unwrap();
    let chapter_one_end = chapter_two - 1;
    let alpha_start = text.find("Alpha arrived.").unwrap();
    let alpha_end = alpha_start + "Alpha arrived.".len();
    let beta_start = text.find("Beta waited.").unwrap();
    let beta_end = beta_start + "Beta waited.".len();
    let gamma_start = text.find("Gamma left.").unwrap();
    let gamma_end = gamma_start + "Gamma left.".len();

    let binding = DocumentAnalysisBinding {
        source_document_id: "fixture:shortrun-structural".to_owned(),
        native_document_id: 7,
        document_revision: 11,
        content_hash: *blake3::hash(text.as_bytes()).as_bytes(),
        analysis_generation: 13,
        source_registry_revision: 17,
        target_registry_revision: 18,
        producer_binary_hash: [23; 32],
        chunker: model("phoenix-chunker/structural-v1", 29),
        dynamic_ner: model("not-run", 31),
        nli: model("not-run", 37),
    };
    let spans = vec![
        span(
            StructuralSpanKind::Paragraph,
            0,
            chapter_one_end,
            0,
            0,
            2,
            "",
            &text,
        ),
        span(
            StructuralSpanKind::Paragraph,
            chapter_two,
            text.len(),
            1,
            2,
            3,
            "",
            &text,
        ),
        span(
            StructuralSpanKind::Chapter,
            0,
            chapter_one_end,
            NO_STRUCTURAL_PARENT as usize,
            0,
            1,
            "Chapter 1: Dawn",
            &text,
        ),
        span(
            StructuralSpanKind::Chapter,
            chapter_two,
            text.len(),
            NO_STRUCTURAL_PARENT as usize,
            1,
            2,
            "Chapter 2: Dusk",
            &text,
        ),
    ];
    let sentences = vec![
        sentence(alpha_start, alpha_end, 0, 0, &text),
        sentence(beta_start, beta_end, 0, 0, &text),
        sentence(gamma_start, gamma_end, 1, 1, &text),
    ];
    let chunks = vec![
        chunk(0, chapter_one_end, 0, 2, 0, 1, 0, &text),
        chunk(chapter_two, text.len(), 2, 3, 1, 2, 1, &text),
    ];
    Fixture {
        text: text.clone(),
        structural: PhoenixStructuralSubstrateV1 {
            schema: STRUCTURAL_SUBSTRATE_CONTRACT.to_owned(),
            binding,
            source_len: text.len() as u32,
            chunks,
            sentences,
            spans,
        },
    }
}

fn model(name: &str, seed: u8) -> AnalysisModelIdentity {
    AnalysisModelIdentity {
        model_id: name.to_owned(),
        artifact_hash: [seed; 32],
        config_hash: [seed.wrapping_add(1); 32],
        runtime_id: "rust-native".to_owned(),
    }
}

fn sentence(
    start: usize,
    end: usize,
    paragraph: u32,
    chapter: u32,
    text: &str,
) -> AnalysisSentenceRecord {
    AnalysisSentenceRecord {
        start: start as u32,
        end: end as u32,
        paragraph_index: paragraph,
        chapter_index: chapter,
        token_count: text[start..end].split_whitespace().count() as u32,
        content_hash: fnv64(&text[start..end]),
        quality: StructuralSentenceQuality::Complete,
        dialogue_hint: StructuralDialogueHint::None,
    }
}

#[allow(clippy::too_many_arguments)]
fn chunk(
    start: usize,
    end: usize,
    sentence_start: u32,
    sentence_end: u32,
    paragraph_start: u32,
    paragraph_end: u32,
    chapter: u32,
    text: &str,
) -> AnalysisChunkRecord {
    AnalysisChunkRecord {
        start: start as u32,
        end: end as u32,
        sentence_start,
        sentence_end,
        paragraph_start,
        paragraph_end,
        chapter_index: chapter,
        token_count: text[start..end].split_whitespace().count() as u32,
        content_hash: fnv64(&text[start..end]),
        dialogue_hint: StructuralDialogueHint::None,
    }
}

#[allow(clippy::too_many_arguments)]
fn span(
    kind: StructuralSpanKind,
    start: usize,
    end: usize,
    parent: usize,
    child_start: usize,
    child_end: usize,
    label: &str,
    text: &str,
) -> AnalysisSpanRecord {
    AnalysisSpanRecord {
        kind,
        start: start as u32,
        end: end as u32,
        parent_index: parent as u32,
        child_start: child_start as u32,
        child_end: child_end as u32,
        token_count: text[start..end].split_whitespace().count() as u32,
        content_hash: fnv64(&text[start..end]),
        label: label.to_owned(),
        dialogue_hint: StructuralDialogueHint::None,
    }
}

fn fnv64(text: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}
