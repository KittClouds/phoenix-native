use crate::build::{build_structural_pages, cohort_hash, structural_page_kinds};
use crate::DocumentProducerError;
use phoenix_analysis_contract::PhoenixStructuralSubstrateV1;
use phoenix_graph_generation_v2::{
    write_generation_new, CapabilityState, ChapterRecord, ChunkRecord, DocumentRecord,
    GenerationWriteAuthority, PageKind, ParagraphRecord, SentenceRecord, SpanRecord,
    StructuralEdgeRecord, VerifiedGraphGenerationV2, GRAPH_GENERATION_V2_EXTENSION,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_NONCE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug)]
pub struct StructuralProducerInput<'a> {
    pub text: &'a str,
    pub structural: &'a PhoenixStructuralSubstrateV1,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StructuralReuseState {
    Produced,
    DurableVerified,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StructuralPageReceipt {
    pub kind: PageKind,
    pub record_count: u64,
    pub page_hash: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StructuralPublicationReceipt {
    pub path: PathBuf,
    pub reuse_state: StructuralReuseState,
    pub capability_state: CapabilityState,
    pub native_document_id: u64,
    pub document_revision: u64,
    pub registry_revision: u64,
    pub producer_generation: u64,
    pub content_hash: [u8; 32],
    pub generation_hash: [u8; 32],
    pub cohort_hash: [u8; 32],
    pub source_coordinate_hash: [u8; 32],
    pub pages: [StructuralPageReceipt; 7],
}

pub struct VerifiedStructuralGeneration {
    generation: VerifiedGraphGenerationV2,
    receipt: StructuralPublicationReceipt,
}

impl VerifiedStructuralGeneration {
    pub fn generation(&self) -> &VerifiedGraphGenerationV2 {
        &self.generation
    }

    pub fn receipt(&self) -> &StructuralPublicationReceipt {
        &self.receipt
    }

    pub fn documents(&self) -> Result<&[DocumentRecord], DocumentProducerError> {
        Ok(self.generation.typed_page(PageKind::Documents)?)
    }

    pub fn chapters(&self) -> Result<&[ChapterRecord], DocumentProducerError> {
        Ok(self.generation.typed_page(PageKind::Chapters)?)
    }

    pub fn paragraphs(&self) -> Result<&[ParagraphRecord], DocumentProducerError> {
        Ok(self.generation.typed_page(PageKind::Paragraphs)?)
    }

    pub fn sentences(&self) -> Result<&[SentenceRecord], DocumentProducerError> {
        Ok(self.generation.typed_page(PageKind::Sentences)?)
    }

    pub fn chunks(&self) -> Result<&[ChunkRecord], DocumentProducerError> {
        Ok(self.generation.typed_page(PageKind::Chunks)?)
    }

    pub fn spans(&self) -> Result<&[SpanRecord], DocumentProducerError> {
        Ok(self.generation.typed_page(PageKind::Spans)?)
    }

    pub fn structural_edges(&self) -> Result<&[StructuralEdgeRecord], DocumentProducerError> {
        Ok(self.generation.typed_page(PageKind::StructuralEdges)?)
    }
}

pub fn publish_or_reuse_structural_generation(
    directory: impl AsRef<Path>,
    input: StructuralProducerInput<'_>,
) -> Result<VerifiedStructuralGeneration, DocumentProducerError> {
    input
        .structural
        .validate()
        .map_err(DocumentProducerError::InvalidStructuralInput)?;
    let binding = &input.structural.binding;
    if input.structural.source_len as usize != input.text.len()
        || blake3::hash(input.text.as_bytes()).as_bytes() != &binding.content_hash
    {
        return Err(DocumentProducerError::SourceBindingMismatch);
    }

    let cohort_hash = cohort_hash(input.structural);
    let directory = directory.as_ref();
    fs::create_dir_all(directory).map_err(|source| DocumentProducerError::PublicationIo {
        path: directory.to_path_buf(),
        source,
    })?;
    let path = generation_path(directory, input.structural, cohort_hash);

    if path.is_file() {
        return open_verified(
            path,
            input.structural,
            cohort_hash,
            StructuralReuseState::DurableVerified,
        );
    }

    let built = build_structural_pages(input.text, input.structural)?;
    let temporary = temporary_path(&path);
    let authority = GenerationWriteAuthority {
        source_document_id_hash: *blake3::hash(binding.source_document_id.as_bytes()).as_bytes(),
        content_hash: binding.content_hash,
        cohort_hash: built.cohort_hash,
        native_document_id: binding.native_document_id,
        document_revision: binding.document_revision,
        registry_revision: binding.target_registry_revision,
        producer_generation: binding.analysis_generation,
        published_generation: binding.analysis_generation,
    };
    let generation = write_generation_new(&temporary, authority, built.pages())?;
    drop(generation);

    match fs::rename(&temporary, &path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists && path.is_file() => {
            let _ = fs::remove_file(&temporary);
            return open_verified(
                path,
                input.structural,
                cohort_hash,
                StructuralReuseState::DurableVerified,
            );
        }
        Err(source) => {
            let _ = fs::remove_file(&temporary);
            return Err(DocumentProducerError::PublicationIo {
                path: path.clone(),
                source,
            });
        }
    }

    open_verified(
        path,
        input.structural,
        cohort_hash,
        StructuralReuseState::Produced,
    )
}

fn open_verified(
    path: PathBuf,
    structural: &PhoenixStructuralSubstrateV1,
    cohort_hash: [u8; 32],
    reuse_state: StructuralReuseState,
) -> Result<VerifiedStructuralGeneration, DocumentProducerError> {
    let generation = VerifiedGraphGenerationV2::open(&path)?;
    let binding = &structural.binding;
    let header = generation.header();
    let expected_source_hash = blake3::hash(binding.source_document_id.as_bytes());
    if header.source_document_id_hash != *expected_source_hash.as_bytes()
        || header.content_hash != binding.content_hash
        || header.cohort_hash != cohort_hash
        || header.native_document_id != binding.native_document_id
        || header.document_revision != binding.document_revision
        || header.registry_revision != binding.target_registry_revision
        || header.producer_generation != binding.analysis_generation
        || header.published_generation != binding.analysis_generation
    {
        return Err(DocumentProducerError::ExistingAuthorityMismatch { path });
    }

    let documents: &[DocumentRecord] = generation.typed_page(PageKind::Documents)?;
    let publication_receipts: &[phoenix_graph_generation_v2::PublicationReceiptRecord] =
        generation.typed_page(PageKind::PublicationReceipts)?;
    if documents.len() != 1
        || publication_receipts.len() != 1
        || documents[0].source_len != structural.source_len
        || documents[0].chunk_count as usize != structural.chunks.len()
        || documents[0].sentence_count as usize != structural.sentences.len()
        || documents[0].span_count as usize != structural.spans.len()
        || generation.resolve_string(documents[0].source_id)? != binding.source_document_id
    {
        return Err(DocumentProducerError::ExistingAuthorityMismatch { path });
    }

    let pages = structural_page_kinds().map(|kind| {
        let descriptor = generation.descriptor(kind);
        StructuralPageReceipt {
            kind,
            record_count: descriptor.count,
            page_hash: descriptor.hash,
        }
    });
    let receipt = StructuralPublicationReceipt {
        path,
        reuse_state,
        capability_state: match reuse_state {
            StructuralReuseState::Produced => CapabilityState::Produced,
            StructuralReuseState::DurableVerified => CapabilityState::DurableVerified,
        },
        native_document_id: binding.native_document_id,
        document_revision: binding.document_revision,
        registry_revision: binding.target_registry_revision,
        producer_generation: binding.analysis_generation,
        content_hash: binding.content_hash,
        generation_hash: header.generation_hash,
        cohort_hash,
        source_coordinate_hash: publication_receipts[0].authority_hash,
        pages,
    };
    Ok(VerifiedStructuralGeneration {
        generation,
        receipt,
    })
}

fn generation_path(
    directory: &Path,
    structural: &PhoenixStructuralSubstrateV1,
    cohort_hash: [u8; 32],
) -> PathBuf {
    let binding = &structural.binding;
    let content = hex_prefix(&binding.content_hash);
    let cohort = hex_prefix(&cohort_hash);
    directory.join(format!(
        "{:016x}-r{}-g{}-{content}-{cohort}.{GRAPH_GENERATION_V2_EXTENSION}",
        binding.native_document_id, binding.document_revision, binding.analysis_generation
    ))
}

fn temporary_path(final_path: &Path) -> PathBuf {
    let nonce = TEMP_NONCE.fetch_add(1, Ordering::Relaxed);
    final_path.with_extension(format!(
        "{}.tmp-{}-{nonce}",
        GRAPH_GENERATION_V2_EXTENSION,
        std::process::id()
    ))
}

fn hex_prefix(hash: &[u8; 32]) -> String {
    let mut text = String::with_capacity(16);
    for byte in &hash[..8] {
        use std::fmt::Write as _;
        let _ = write!(text, "{byte:02x}");
    }
    text
}
