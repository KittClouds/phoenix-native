use anyhow::{bail, Context, Result};
use phoenix_analysis_contract::{
    open_analysis_artifact, open_nli_artifact, open_producer_coordinator, open_structural_artifact,
};
use phoenix_app_core::{
    AtlasCapabilityCount, AtlasCapabilityState, AtlasRunReceiptV1, KernelCommand,
    NativeProducerRuntimeConfig, PhoenixKernel,
};
use phoenix_graph_generation_v2::{PageKind, VerifiedGraphGenerationV2};
use phoenix_workspace::ContentHash;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

mod shadow_v2;

fn main() -> Result<()> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    match args.first().and_then(|arg| arg.to_str()) {
        Some("seed-and-publish") if args.len() == 8 => seed_and_publish(
            Path::new(&args[1]),
            Path::new(&args[2]),
            Path::new(&args[3]),
            args[4].to_string_lossy().into_owned(),
            PathBuf::from(&args[5]),
            PathBuf::from(&args[6]),
            PathBuf::from(&args[7]),
        ),
        Some("verify") if args.len() == 4 => verify(
            Path::new(&args[1]),
            Path::new(&args[2]),
            &args[3].to_string_lossy(),
        ),
        Some("run-pipeline") if args.len() == 7 => run_pipeline(
            Path::new(&args[1]),
            Path::new(&args[2]),
            args[3].to_string_lossy().into_owned(),
            PathBuf::from(&args[4]),
            PathBuf::from(&args[5]),
            PathBuf::from(&args[6]),
        ),
        Some("compare-semantics") if args.len() == 5 => compare_semantics(
            Path::new(&args[1]),
            Path::new(&args[2]),
            Path::new(&args[3]),
            Path::new(&args[4]),
        ),
        Some("shadow-v2-produce") if args.len() == 8 => shadow_v2::produce(
            Path::new(&args[1]),
            parse_entry_id(&args[2])?,
            Path::new(&args[3]),
            Path::new(&args[4]),
            Path::new(&args[5]),
            Path::new(&args[6]),
            &args[7].to_string_lossy(),
        ),
        Some("shadow-v2-verify") if args.len() == 7 => shadow_v2::verify(
            Path::new(&args[1]),
            parse_entry_id(&args[2])?,
            Path::new(&args[3]),
            Path::new(&args[4]),
            Path::new(&args[5]),
            Path::new(&args[6]),
        ),
        Some("inspect-topology") if args.len() == 4 => inspect_topology(
            Path::new(&args[1]),
            Path::new(&args[2]),
            Path::new(&args[3]),
        ),
        _ => bail!(
            "usage: phoenix-analysis-proof seed-and-publish <workspace> <publication-root> \
             <document> <source-document-id> <producer> <ner-model-root> <nli-model-root>\n\
             or: phoenix-analysis-proof verify <workspace> <publication-root> <blake3>\n\
             or: phoenix-analysis-proof run-pipeline <workspace> <publication-root> \
             <source-document-id> <producer> <ner-model-root> <nli-model-root>\n\
             or: phoenix-analysis-proof compare-semantics \
             <analysis-a> <nli-a> <analysis-b> <nli-b>\n\
             or: phoenix-analysis-proof shadow-v2-produce <workspace> <entry-id> \
             <analysis> <structural> <coordinator> <output-root> <run-label>\n\
             or: phoenix-analysis-proof shadow-v2-verify <workspace> <entry-id> \
             <analysis> <structural> <coordinator> <entity-generation>\n\
             or: phoenix-analysis-proof inspect-topology <analysis> <coordinator> <generation>"
        ),
    }
}

fn inspect_topology(
    analysis_path: &Path,
    coordinator_path: &Path,
    generation_path: &Path,
) -> Result<()> {
    let analysis = open_analysis_artifact(analysis_path)
        .with_context(|| format!("open analysis {}", analysis_path.display()))?;
    let structural_path = analysis_path.with_file_name(
        analysis_path
            .file_name()
            .and_then(|name| name.to_str())
            .context("analysis path has no UTF-8 file name")?
            .replace(".analysis.pnaa", ".structural.pnss"),
    );
    let structural = open_structural_artifact(&structural_path)
        .with_context(|| format!("open structural {}", structural_path.display()))?;
    let coordinator = open_producer_coordinator(coordinator_path)
        .with_context(|| format!("open coordinator {}", coordinator_path.display()))?;
    let generation = VerifiedGraphGenerationV2::open(generation_path)
        .with_context(|| format!("open generation {}", generation_path.display()))?;
    let coordinator = coordinator.coordinator();
    println!("document_id={}", coordinator.binding.native_document_id);
    println!(
        "document_revision={}",
        coordinator.binding.document_revision
    );
    println!(
        "analysis_generation={}",
        coordinator.binding.analysis_generation
    );
    println!("candidate_evidence={}", coordinator.evidence_bindings.len());
    println!(
        "analysis_mentions={}",
        analysis.analysis().ner.mentions.len()
    );
    println!(
        "accepted_mentions={}",
        analysis
            .analysis()
            .ner
            .mentions
            .iter()
            .filter(|mention| mention.accepted)
            .count()
    );
    println!(
        "contextual_evidence_bindings={}",
        coordinator.contextual_evidence_bindings.len()
    );
    let chunks = &structural.structural().chunks;
    let mut expected_contextual_bindings = 0_usize;
    let mut per_chunk = vec![BTreeMap::<u64, u64>::new(); chunks.len()];
    for mention in &analysis.analysis().ner.mentions {
        let chunk_index = chunks.partition_point(|chunk| chunk.end <= mention.start);
        let chunk = chunks
            .get(chunk_index)
            .context("exported mention has no structural chunk")?;
        if chunk.start > mention.start || chunk.end < mention.end {
            bail!("exported mention crosses its structural chunk");
        }
        per_chunk[chunk_index]
            .entry(mention.entity_id)
            .and_modify(|mention_id| *mention_id = (*mention_id).min(mention.mention_id))
            .or_insert(mention.mention_id);
    }
    for entities in &per_chunk {
        expected_contextual_bindings = expected_contextual_bindings
            .checked_add(
                entities
                    .len()
                    .saturating_mul(entities.len().saturating_sub(1))
                    / 2,
            )
            .context("contextual evidence count overflow")?;
    }
    println!("expected_contextual_bindings={expected_contextual_bindings}");
    for capability in &coordinator.capabilities {
        println!(
            "producer={:?}:{:?}:{}",
            capability.product,
            capability.state,
            capability
                .output_count
                .map_or_else(|| "unsupported".to_owned(), |count| count.to_string())
        );
    }
    for page in PageKind::ALL {
        let descriptor = generation.descriptor(page);
        println!(
            "page={page:?}:count={}:bytes={}:hash={}",
            descriptor.count,
            descriptor.length,
            descriptor
                .hash
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        );
    }
    Ok(())
}

fn parse_entry_id(value: &std::ffi::OsStr) -> Result<u64> {
    value
        .to_string_lossy()
        .parse::<u64>()
        .context("entry ID must be an unsigned integer")
}

fn run_pipeline(
    workspace_path: &Path,
    publication_root: &Path,
    source_document_id: String,
    producer: PathBuf,
    ner_model_root: PathBuf,
    nli_model_root: PathBuf,
) -> Result<()> {
    let kernel = PhoenixKernel::start_production_at_root(
        workspace_path.to_path_buf(),
        publication_root.to_path_buf(),
    )?;
    let config = NativeProducerRuntimeConfig {
        producer_executable: producer,
        ner_model_root,
        nli_model_root,
        source_document_id: Some(source_document_id),
        max_nli_candidates: 65_536,
    };
    let command = kernel.run_active_document_pipeline_with(&config)?;
    let snapshot = kernel.snapshot()?;
    let lease = snapshot
        .active_document_lease
        .context("pipeline active document is unavailable")?;
    let analysis = snapshot
        .analysis_publication
        .context("pipeline analysis receipt is unavailable")?;
    let graph = match command.outcome {
        phoenix_app_core::KernelOutcome::GraphRebuilt(receipt) => receipt,
        other => bail!("pipeline returned the wrong outcome: {other:?}"),
    };
    print_receipt("pipeline", lease.content_hash, &analysis);
    println!("scene_generation={}", graph.publication.generation_id);
    println!("scene_nodes={}", graph.publication.node_count);
    println!("scene_edges={}", graph.publication.edge_count);
    println!(
        "scene_archive={}",
        hex(&graph.publication.archive_cohort_hash)
    );
    println!(
        "scene_product_index={}",
        hex(&graph.publication.product_index_hash)
    );
    println!("scene_compile_micros={}", graph.compile.compile_micros);
    let control = kernel.atlas_control_snapshot()?;
    let atlas_run = control
        .last_run
        .context("pipeline Atlas run receipt is unavailable")?;
    let atlas_run_hash = control
        .last_run_hash
        .context("pipeline Atlas run receipt hash is unavailable")?;
    if control.last_run_restored {
        bail!("new pipeline receipt was incorrectly marked as restored");
    }
    println!("pipeline_atlas_receipt_hash={}", hex(&atlas_run_hash));
    print_atlas_run("pipeline", &atlas_run);
    kernel.shutdown()?;
    Ok(())
}

fn seed_and_publish(
    workspace_path: &Path,
    publication_root: &Path,
    document_path: &Path,
    source_document_id: String,
    producer: PathBuf,
    ner_model_root: PathBuf,
    nli_model_root: PathBuf,
) -> Result<()> {
    if workspace_path.exists()
        || workspace_path
            .parent()
            .is_some_and(|parent| parent.join("analysis-authority-v1").exists())
    {
        bail!("proof workspace is not fresh: {}", workspace_path.display());
    }
    let text = std::fs::read_to_string(document_path)
        .with_context(|| format!("read exact document {}", document_path.display()))?;
    let kernel = PhoenixKernel::start_production_at_root(
        workspace_path.to_path_buf(),
        publication_root.to_path_buf(),
    )?;
    let lease = kernel
        .snapshot()?
        .active_document_lease
        .context("seeded active document is unavailable")?;
    kernel.execute(KernelCommand::SaveDocument {
        lease: lease.token(),
        content: Arc::from(text),
    })?;
    let receipt = kernel.analyze_active_document_with(
        2,
        &NativeProducerRuntimeConfig {
            producer_executable: producer,
            ner_model_root,
            nli_model_root,
            source_document_id: Some(source_document_id),
            max_nli_candidates: 65_536,
        },
    )?;
    let snapshot = kernel.snapshot()?;
    let lease = snapshot
        .active_document_lease
        .context("committed active document is unavailable")?;
    print_receipt("published", lease.content_hash, &receipt);
    print_binding(
        &snapshot
            .nli_analysis
            .context("published NLI authority is unavailable")?
            .binding,
    );
    kernel.shutdown()?;
    Ok(())
}

fn verify(workspace_path: &Path, publication_root: &Path, expected_hash: &str) -> Result<()> {
    let kernel = PhoenixKernel::start_production_at_root(
        workspace_path.to_path_buf(),
        publication_root.to_path_buf(),
    )?;
    let snapshot = kernel.snapshot()?;
    let lease = snapshot
        .active_document_lease
        .context("reopened active document is unavailable")?;
    if lease.content_hash.to_hex() != expected_hash {
        bail!(
            "fresh-process document hash mismatch: expected {expected_hash}, got {}",
            lease.content_hash.to_hex()
        );
    }
    let receipt = snapshot
        .analysis_publication
        .context("fresh process did not restore bound analysis authority")?;
    let nli = snapshot
        .nli_analysis
        .context("fresh process did not restore candidate-only NLI artifact")?;
    if receipt.promotion_count != 0 || nli.promotion_count != 0 {
        bail!("candidate-only NLI invariant was violated");
    }
    print_receipt("reopened", lease.content_hash, &receipt);
    print_binding(&nli.binding);
    let control = kernel.atlas_control_snapshot()?;
    let atlas_run = control
        .last_run
        .context("fresh process did not restore matching Atlas run authority")?;
    let atlas_run_hash = control
        .last_run_hash
        .context("fresh process did not restore the Atlas run receipt hash")?;
    if !control.last_run_restored {
        bail!("fresh-process Atlas run receipt was not marked as restored");
    }
    if atlas_run.authority.content_hash != lease.content_hash.0 {
        bail!("fresh-process Atlas run receipt does not match the active document");
    }
    println!("reopened_atlas_receipt_hash={}", hex(&atlas_run_hash));
    print_atlas_run("reopened", &atlas_run);
    kernel.shutdown()?;
    Ok(())
}

fn print_atlas_run(side: &str, receipt: &AtlasRunReceiptV1) {
    let resources = receipt.resources;
    println!("{side}_atlas_run_id={}", receipt.run_id);
    println!("{side}_atlas_document_id={}", receipt.authority.document_id);
    println!(
        "{side}_atlas_document_revision={}",
        receipt.authority.document_revision
    );
    println!(
        "{side}_atlas_document_hash={}",
        hex(&receipt.authority.content_hash)
    );
    println!(
        "{side}_atlas_registry_revision={}",
        receipt.authority.registry_revision
    );
    println!(
        "{side}_atlas_previous_generation={}",
        receipt
            .authority
            .previous_generation
            .map_or_else(|| "none".to_owned(), |generation| generation.to_string())
    );
    println!(
        "{side}_atlas_published_generation={}",
        receipt.authority.published_generation
    );
    println!(
        "{side}_atlas_analysis_entities={}",
        capability(resources.analysis_entities)
    );
    println!(
        "{side}_atlas_analysis_chunks={}",
        capability(resources.analysis_chunks)
    );
    println!("{side}_atlas_scene_chunks={}", resources.scene_chunks);
    println!("{side}_atlas_sentences={}", capability(resources.sentences));
    println!(
        "{side}_atlas_canonical_entities={}",
        resources.canonical_entities
    );
    println!(
        "{side}_atlas_analysis_mentions={}",
        capability(resources.analysis_mentions)
    );
    println!(
        "{side}_atlas_resident_anchors={}",
        resources.resident_verified_anchors
    );
    println!(
        "{side}_atlas_nli_candidates={}",
        capability(resources.nli_candidates)
    );
    println!(
        "{side}_atlas_nli_adjudications={}",
        capability(resources.nli_adjudications)
    );
    println!(
        "{side}_atlas_promotions={}",
        capability(receipt.semantics.promotions)
    );
    println!("{side}_atlas_graph_nodes={}", resources.graph_nodes);
    println!("{side}_atlas_graph_edges={}", resources.graph_edges);
    println!(
        "{side}_atlas_graph_accepted_edges={}",
        receipt.graph_reviews.accepted_edges
    );
    println!(
        "{side}_atlas_graph_proposed_edges={}",
        receipt.graph_reviews.proposed_edges
    );
    println!(
        "{side}_atlas_decisions_accepted={}",
        capability(receipt.decisions.accepted)
    );
    println!("{side}_atlas_total_micros={}", receipt.timings.total_micros);
    println!(
        "{side}_atlas_analysis_total_micros={}",
        capability(receipt.timings.analysis_total_micros)
    );
    println!(
        "{side}_atlas_chunker_micros={}",
        capability(receipt.timings.chunker_micros)
    );
    println!(
        "{side}_atlas_dynamic_ner_micros={}",
        capability(receipt.timings.dynamic_ner_micros)
    );
    println!(
        "{side}_atlas_nli_load_micros={}",
        capability(receipt.timings.nli_load_micros)
    );
    println!(
        "{side}_atlas_nli_adjudication_micros={}",
        capability(receipt.timings.nli_adjudication_micros)
    );
    println!(
        "{side}_atlas_compiler_micros={}",
        receipt.timings.compiler_micros
    );
    println!(
        "{side}_atlas_publisher_micros={}",
        receipt.timings.publisher_micros
    );
    println!(
        "{side}_atlas_command_high_water={}->{} / {}",
        receipt.queues.command_high_water_before,
        receipt.queues.command_high_water_after,
        receipt.queues.command_capacity
    );
    println!(
        "{side}_atlas_event_high_water={}->{} / {}",
        receipt.queues.event_high_water_before,
        receipt.queues.event_high_water_after,
        receipt.queues.event_capacity
    );
    println!(
        "{side}_atlas_reuse={:?}/{:?}/{:?}/{:?}",
        receipt.reuse.source,
        receipt.reuse.analysis,
        receipt.reuse.compiler,
        receipt.reuse.publisher
    );
    for span in &receipt.spans {
        println!(
            "{side}_atlas_span={:?}:{}:parent={}:{}us",
            span.kind,
            span.span_id,
            span.parent_span_id
                .map_or_else(|| "none".to_owned(), |parent| parent.to_string()),
            span.elapsed_micros
        );
    }
}

fn capability(value: AtlasCapabilityCount) -> String {
    match (value.state, value.count) {
        (AtlasCapabilityState::Produced, Some(count)) => count.to_string(),
        (AtlasCapabilityState::Unsupported, None) => "unsupported".to_owned(),
        (AtlasCapabilityState::NotRun, None) => "not_run".to_owned(),
        _ => "invalid".to_owned(),
    }
}

fn compare_semantics(
    analysis_a_path: &Path,
    nli_a_path: &Path,
    analysis_b_path: &Path,
    nli_b_path: &Path,
) -> Result<()> {
    let analysis_a = open_analysis_artifact(analysis_a_path)?;
    let analysis_b = open_analysis_artifact(analysis_b_path)?;
    let nli_a = open_nli_artifact(nli_a_path)?;
    let nli_b = open_nli_artifact(nli_b_path)?;
    let left = analysis_a.analysis();
    let right = analysis_b.analysis();
    let left_nli = nli_a.nli();
    let right_nli = nli_b.nli();

    if left.ner.binding.content_hash != right.ner.binding.content_hash {
        bail!("analysis document hashes differ");
    }
    if left.ner.entities != right.ner.entities {
        bail!("NER entity semantics differ");
    }
    if left.ner.mentions != right.ner.mentions {
        bail!("NER mention semantics differ");
    }
    if left_nli.nli_candidates != right_nli.nli_candidates {
        bail!("NLI candidate semantics differ");
    }
    if left_nli.nli_adjudications != right_nli.nli_adjudications {
        bail!("NLI adjudication semantics differ");
    }
    if left_nli.promotion_count != right_nli.promotion_count {
        bail!("NLI promotion counts differ");
    }

    println!("semantic_parity=true");
    println!("entities={}", left.ner.entities.len());
    println!("mentions={}", left.ner.mentions.len());
    println!("nli_candidates={}", left_nli.nli_candidates.len());
    println!("nli_adjudications={}", left_nli.nli_adjudications.len());
    println!("promotions={}", left_nli.promotion_count);
    print_stage_times("left", &left.ner.receipt);
    print_stage_times("right", &right.ner.receipt);
    Ok(())
}

fn print_stage_times(side: &str, receipt: &phoenix_analysis_contract::AnalysisStageReceipt) {
    println!("{side}_chunker_micros={}", receipt.chunker_micros);
    println!("{side}_dynamic_ner_micros={}", receipt.dynamic_ner_micros);
    println!("{side}_nli_load_micros={}", receipt.nli_load_micros);
    println!(
        "{side}_nli_adjudication_micros={}",
        receipt.nli_adjudication_micros
    );
}

fn print_binding(binding: &phoenix_analysis_contract::DocumentAnalysisBinding) {
    println!("source_document_id={}", binding.source_document_id);
    println!(
        "source_registry_revision={}",
        binding.source_registry_revision
    );
    println!(
        "target_registry_revision={}",
        binding.target_registry_revision
    );
    println!("producer_binary={}", hex(&binding.producer_binary_hash));
    for (lane, model) in [
        ("chunker", &binding.chunker),
        ("dynamic_ner", &binding.dynamic_ner),
        ("nli", &binding.nli),
    ] {
        println!("{lane}_model_id={}", model.model_id);
        println!("{lane}_artifact={}", hex(&model.artifact_hash));
        println!("{lane}_config={}", hex(&model.config_hash));
        println!("{lane}_runtime={}", model.runtime_id);
    }
}

fn print_receipt(
    phase: &str,
    content_hash: ContentHash,
    receipt: &phoenix_app_core::AnalysisPublicationReceipt,
) {
    println!("phase={phase}");
    println!("document_blake3={}", content_hash.to_hex());
    println!("document_revision={}", receipt.document_revision);
    println!("analysis_generation={}", receipt.analysis_generation);
    println!("registry_revision={}", receipt.registry_revision);
    println!("entities={}", receipt.entity_count);
    println!("mentions={}", receipt.mention_count);
    println!("nli_candidates={}", receipt.nli_candidate_count);
    println!("nli_adjudications={}", receipt.nli_adjudication_count);
    println!("promotions={}", receipt.promotion_count);
    println!("analysis_artifact={}", hex(&receipt.analysis_artifact_hash));
    println!("nli_artifact={}", hex(&receipt.nli_artifact_hash));
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(DIGITS[(byte >> 4) as usize] as char);
        output.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    output
}
