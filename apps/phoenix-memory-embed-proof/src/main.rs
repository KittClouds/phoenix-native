use std::collections::HashMap;
use std::env;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{bail, Context, Result};
use phoenix_embed::{
    default_ort_dylib_path, workspace_root, OrtTextEmbedConfig, OrtTextEmbedder,
    TextEmbeddingInputPrefix,
};
use phoenix_memory_contract::{
    ContentUnitKind, ContentUnitRecord, ConversationInput, DocumentChunkInput, DocumentInput,
    DocumentRevisionRecord, MixedSourceBuilder, PageKindV3, ParticipantRole, SourceKind, TurnInput,
    TurnRecord, VerifiedGraphGenerationV3,
};
use phoenix_memory_embeddings::{
    write_embedding_pages_new, EmbeddingPageExpectation, EmbeddingPageWriteAuthority,
    EmbeddingRowV1, VerifiedEmbeddingPagesV1, ROW_FLAG_NORMALIZED,
};
use serde::Serialize;

const MODEL_ID: &str = "onnx-community/embeddinggemma-300m-ONNX";
const QUERY_PREFIX: &str = "task: search result | query: ";
const DOCUMENT_PREFIX: &str = "title: none | text: ";
const DEFAULT_MODEL_ROOT: &str = "D:\\phoenix-models\\embeddinggemma-300m-ONNX";
const DEFAULT_OUTPUT: &str = "D:\\phoenix-memory-embedding-proof\\embeddinggemma-pages-v1.phxe1";

fn main() {
    if let Err(error) = run() {
        eprintln!("phoenix-memory-embed-proof: {error:#}");
        std::process::exit(2);
    }
}

fn run() -> Result<()> {
    let args = Args::parse(env::args().skip(1))?;
    if env::var_os("ORT_DYLIB_PATH").is_none() {
        if let Some(path) = default_ort_dylib_path(&workspace_root()) {
            // SAFETY: this single-threaded CLI sets the process runtime path
            // before ONNX Runtime is initialized.
            unsafe { env::set_var("ORT_DYLIB_PATH", path) };
        }
    }
    let output_parent = args
        .output
        .parent()
        .context("output must have a parent directory")?;
    fs::create_dir_all(output_parent)
        .with_context(|| format!("create {}", output_parent.display()))?;
    let generation_path = output_parent.join("embeddinggemma-cohort.phxgg3");
    if args.replace {
        remove_if_exists(&args.output)?;
        remove_if_exists(&generation_path)?;
    } else if args.output.exists() || generation_path.exists() {
        bail!("output exists; pass --replace to replace proof artifacts");
    }

    let fixture_started = Instant::now();
    let generation = write_fixture_generation(&generation_path)?;
    let inputs = collect_embedding_inputs(&generation)?;
    let fixture_micros = micros(fixture_started.elapsed());

    let model_asset_hash = hash_model_assets(&args.model_root)?;
    let model_identity_hash = *blake3::hash(MODEL_ID.as_bytes()).as_bytes();
    let config_hash = *blake3::hash(
        b"embeddinggemma/q4/native768/max2048/query-prefix-v1/document-prefix-v1/sentence-embedding",
    )
    .as_bytes();

    let mut texts = Vec::with_capacity(inputs.len() + 1);
    texts.push(format!("{QUERY_PREFIX}{}", args.query));
    texts.extend(
        inputs
            .iter()
            .map(|input| format!("{DOCUMENT_PREFIX}{}", input.text)),
    );
    let config = OrtTextEmbedConfig {
        input_prefix: TextEmbeddingInputPrefix::None,
        ..OrtTextEmbedConfig::embedding_gemma_document(args.model_root.clone())
    };
    let load_started = Instant::now();
    let embedder = OrtTextEmbedder::load(&config)?;
    let load_micros = micros(load_started.elapsed());
    let embed_started = Instant::now();
    let embedded = embedder.embed_texts_flat(&texts)?;
    let embedding_micros = micros(embed_started.elapsed());
    if embedded.rows() != texts.len() || embedded.dims() != 768 {
        bail!(
            "unexpected embedding shape: {}x{} for {} inputs",
            embedded.rows(),
            embedded.dims(),
            texts.len()
        );
    }
    let query = embedded.row(0).context("query row missing")?;
    let mut vectors = Vec::with_capacity(inputs.len() * embedded.dims());
    for index in 1..embedded.rows() {
        vectors.extend_from_slice(embedded.row(index).context("document row missing")?);
    }
    let rows = build_rows(&inputs, embedded.dims() as u32)?;

    let write_started = Instant::now();
    let written = write_embedding_pages_new(
        &args.output,
        EmbeddingPageWriteAuthority {
            generation_hash: generation.header().generation_hash,
            source_set_hash: generation.header().source_set_hash,
            model_identity_hash,
            model_asset_hash,
            config_hash,
            dimension: embedded.dims() as u32,
        },
        &rows,
        &vectors,
    )?;
    let write_micros = micros(write_started.elapsed());
    let artifact_hash = written.header().artifact_hash;
    drop(written);

    let open_started = Instant::now();
    let reopened = VerifiedEmbeddingPagesV1::open_expected(
        &args.output,
        EmbeddingPageExpectation {
            generation_hash: Some(generation.header().generation_hash),
            source_set_hash: Some(generation.header().source_set_hash),
            model_identity_hash: Some(model_identity_hash),
            config_hash: Some(config_hash),
        },
    )?;
    let open_micros = micros(open_started.elapsed());
    let ranking = rank(query, &reopened, &inputs)?;
    let top = ranking.first().context("ranking is empty")?;
    if !top.text.contains("Mars") {
        bail!(
            "semantic smoke failed: expected Mars evidence first, got {:?}",
            top.text
        );
    }

    let receipt = ProofReceipt {
        contract: "phoenix.embedding-pages/v1",
        model_id: MODEL_ID,
        model_root: args.model_root.display().to_string(),
        execution_provider: config.execution_provider.label(),
        generation_path: generation_path.display().to_string(),
        generation_hash: hex(generation.header().generation_hash),
        source_set_hash: hex(generation.header().source_set_hash),
        output_path: args.output.display().to_string(),
        artifact_hash: hex(artifact_hash),
        model_identity_hash: hex(model_identity_hash),
        model_asset_hash: hex(model_asset_hash),
        config_hash: hex(config_hash),
        row_count: reopened.rows()?.len(),
        dimension: reopened.header().dimension,
        artifact_bytes: reopened.header().total_len,
        fixture_micros,
        model_load_micros: load_micros,
        embedding_micros,
        write_micros,
        verified_mmap_open_micros: open_micros,
        top_match: top.clone(),
        top_three: ranking.into_iter().take(3).collect(),
    };
    println!("{}", serde_json::to_string_pretty(&receipt)?);
    Ok(())
}

fn write_fixture_generation(path: &Path) -> Result<VerifiedGraphGenerationV3> {
    let paragraphs = [
        "Venus resembles Earth in size but is wrapped in a dense atmosphere.",
        "Mars is known as the Red Planet because iron minerals oxidize in its soil.",
        "Jupiter is the largest planet and carries a persistent red storm.",
        "Saturn is distinguished by a broad system of icy rings.",
    ];
    let mut text = String::new();
    let mut chunks = Vec::with_capacity(paragraphs.len());
    for (ordinal, paragraph) in paragraphs.iter().enumerate() {
        if !text.is_empty() {
            text.push_str("\n\n");
        }
        let start = text.len() as u32;
        text.push_str(paragraph);
        let end = text.len() as u32;
        chunks.push(DocumentChunkInput {
            start,
            end,
            sentence_start: ordinal as u32,
            sentence_end: ordinal as u32 + 1,
            paragraph_start: ordinal as u32,
            paragraph_end: ordinal as u32 + 1,
            chapter_index: 0,
            token_count: paragraph.split_whitespace().count() as u32,
            flags: 0,
        });
    }

    MixedSourceBuilder::new(b"phoenix/embeddinggemma-cli-proof")
        .generations(1, 1, 1)
        .add_document(DocumentInput::current(
            b"embeddinggemma-document".to_vec(),
            1,
            "Proof/Planets",
            text,
            chunks,
            1,
        ))
        .add_conversation(ConversationInput {
            external_id: b"embeddinggemma-conversation".to_vec(),
            started_at_millis: 1_720_000_000_000,
            ended_at_millis: 1_720_000_001_000,
            turns: vec![
                TurnInput {
                    external_id: b"turn-user".to_vec(),
                    ordinal: 0,
                    role: ParticipantRole::User,
                    event_time_millis: 1_720_000_000_000,
                    reply_to_ordinal: None,
                    actor_entity_id: 0,
                    model_identity_index: None,
                    content: "Which world is called the Red Planet?".into(),
                    flags: 0,
                },
                TurnInput {
                    external_id: b"turn-assistant".to_vec(),
                    ordinal: 1,
                    role: ParticipantRole::Assistant,
                    event_time_millis: 1_720_000_001_000,
                    reply_to_ordinal: Some(0),
                    actor_entity_id: 0,
                    model_identity_index: None,
                    content: "Mars is commonly called the Red Planet.".into(),
                    flags: 0,
                },
            ],
        })
        .prepare()?
        .write(path)
        .map_err(Into::into)
}

#[derive(Clone)]
struct EmbeddingInput {
    subject_id: u64,
    source_id: u64,
    source_kind: SourceKind,
    content_kind: ContentUnitKind,
    content_hash: [u8; 32],
    start: u32,
    end: u32,
    ordinal: u32,
    text: String,
}

fn collect_embedding_inputs(generation: &VerifiedGraphGenerationV3) -> Result<Vec<EmbeddingInput>> {
    let documents =
        generation.typed_page::<DocumentRevisionRecord>(PageKindV3::DocumentRevisions)?;
    let turns = generation.typed_page::<TurnRecord>(PageKindV3::Turns)?;
    let units = generation.typed_page::<ContentUnitRecord>(PageKindV3::ContentUnits)?;
    let mut document_text = HashMap::with_capacity(documents.len());
    for document in documents {
        document_text.insert(
            document.document_id,
            generation.resolve_source_text(document.content)?,
        );
    }
    let mut turn_text = HashMap::with_capacity(turns.len());
    for turn in turns {
        turn_text.insert(turn.id, generation.resolve_source_text(turn.content)?);
    }

    let mut inputs = Vec::new();
    for unit in units {
        let kind = ContentUnitKind::from_raw(unit.kind).context("invalid content unit kind")?;
        let (source_kind, text) = match kind {
            ContentUnitKind::DynamicChunk => {
                let owner = document_text
                    .get(&unit.owner_id)
                    .context("dynamic chunk owner missing")?;
                (
                    SourceKind::WorkspaceDocument,
                    slice_source(owner, unit.start, unit.end)?,
                )
            }
            ContentUnitKind::Turn => {
                let owner = turn_text
                    .get(&unit.owner_id)
                    .context("turn owner missing")?;
                (
                    SourceKind::Conversation,
                    slice_source(owner, unit.start, unit.end)?,
                )
            }
            _ => continue,
        };
        inputs.push(EmbeddingInput {
            subject_id: unit.id,
            source_id: unit.source_id,
            source_kind,
            content_kind: kind,
            content_hash: unit.content_hash,
            start: unit.start,
            end: unit.end,
            ordinal: unit.ordinal,
            text: text.to_owned(),
        });
    }
    inputs.sort_unstable_by_key(|input| input.subject_id);
    if inputs.is_empty() {
        bail!("fixture produced no embeddable content units");
    }
    Ok(inputs)
}

fn slice_source(text: &str, start: u32, end: u32) -> Result<&str> {
    text.get(start as usize..end as usize)
        .context("content unit range is not a UTF-8 boundary")
}

fn build_rows(inputs: &[EmbeddingInput], dimension: u32) -> Result<Vec<EmbeddingRowV1>> {
    inputs
        .iter()
        .enumerate()
        .map(|(index, input)| {
            Ok(EmbeddingRowV1 {
                subject_id: input.subject_id,
                source_id: input.source_id,
                content_hash: input.content_hash,
                vector_start: (index as u64)
                    .checked_mul(u64::from(dimension))
                    .context("vector offset overflow")?,
                source_start: input.start,
                source_end: input.end,
                ordinal: input.ordinal,
                dimension,
                source_kind: input.source_kind as u16,
                content_kind: input.content_kind as u16,
                flags: ROW_FLAG_NORMALIZED,
                reserved: [0; 2],
            })
        })
        .collect()
}

#[derive(Clone, Debug, Serialize)]
struct RankedMatch {
    subject_id: u64,
    source_kind: &'static str,
    score: f32,
    text: String,
}

fn rank(
    query: &[f32],
    pages: &VerifiedEmbeddingPagesV1,
    inputs: &[EmbeddingInput],
) -> Result<Vec<RankedMatch>> {
    let mut ranking = Vec::with_capacity(inputs.len());
    for (index, input) in inputs.iter().enumerate() {
        let vector = pages.vector(index)?.context("verified vector missing")?;
        ranking.push(RankedMatch {
            subject_id: input.subject_id,
            source_kind: match input.source_kind {
                SourceKind::WorkspaceDocument => "document",
                SourceKind::Conversation => "conversation",
            },
            score: dot(query, vector),
            text: input.text.clone(),
        });
    }
    ranking.sort_unstable_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.subject_id.cmp(&right.subject_id))
    });
    Ok(ranking)
}

fn dot(left: &[f32], right: &[f32]) -> f32 {
    left.iter()
        .zip(right)
        .map(|(left, right)| left * right)
        .sum()
}

fn hash_model_assets(root: &Path) -> Result<[u8; 32]> {
    let mut hasher = blake3::Hasher::new();
    for relative in [
        "tokenizer.json",
        "tokenizer_config.json",
        "config.json",
        "onnx/model_q4.onnx",
        "onnx/model_q4.onnx_data",
    ] {
        hasher.update(&(relative.len() as u64).to_le_bytes());
        hasher.update(relative.as_bytes());
        let path = root.join(relative);
        let mut file = File::open(&path).with_context(|| format!("open {}", path.display()))?;
        let mut buffer = vec![0_u8; 1 << 20];
        loop {
            let read = file
                .read(&mut buffer)
                .with_context(|| format!("read {}", path.display()))?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
    }
    Ok(*hasher.finalize().as_bytes())
}

fn remove_if_exists(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("remove {}", path.display())),
    }
}

fn micros(duration: std::time::Duration) -> u64 {
    duration.as_micros().min(u128::from(u64::MAX)) as u64
}

fn hex(bytes: [u8; 32]) -> String {
    const TABLE: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    for byte in bytes {
        output.push(TABLE[(byte >> 4) as usize] as char);
        output.push(TABLE[(byte & 0x0f) as usize] as char);
    }
    output
}

#[derive(Serialize)]
struct ProofReceipt {
    contract: &'static str,
    model_id: &'static str,
    model_root: String,
    execution_provider: &'static str,
    generation_path: String,
    generation_hash: String,
    source_set_hash: String,
    output_path: String,
    artifact_hash: String,
    model_identity_hash: String,
    model_asset_hash: String,
    config_hash: String,
    row_count: usize,
    dimension: u32,
    artifact_bytes: u64,
    fixture_micros: u64,
    model_load_micros: u64,
    embedding_micros: u64,
    write_micros: u64,
    verified_mmap_open_micros: u64,
    top_match: RankedMatch,
    top_three: Vec<RankedMatch>,
}

struct Args {
    model_root: PathBuf,
    output: PathBuf,
    query: String,
    replace: bool,
}

impl Args {
    fn parse(arguments: impl Iterator<Item = String>) -> Result<Self> {
        let mut model_root = PathBuf::from(DEFAULT_MODEL_ROOT);
        let mut output = PathBuf::from(DEFAULT_OUTPUT);
        let mut query = "Which planet is known as the Red Planet?".to_owned();
        let mut replace = false;
        let mut arguments = arguments;
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--model-root" => {
                    model_root =
                        PathBuf::from(arguments.next().context("--model-root requires a path")?);
                }
                "--output" => {
                    output = PathBuf::from(arguments.next().context("--output requires a path")?);
                }
                "--query" => {
                    query = arguments.next().context("--query requires text")?;
                }
                "--replace" => replace = true,
                "--help" | "-h" => {
                    println!(
                        "phoenix-memory-embed-proof [--model-root <dir>] [--output <file>] \
                         [--query <text>] [--replace]"
                    );
                    std::process::exit(0);
                }
                _ => bail!("unknown option {argument:?}"),
            }
        }
        Ok(Self {
            model_root,
            output,
            query,
            replace,
        })
    }
}
