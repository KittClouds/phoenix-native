use anyhow::{bail, Context, Result};
use phoenix_analysis_contract::{open_analysis_artifact, open_nli_artifact};
use phoenix_app_core::{KernelCommand, LegacyAnalysisAdapterConfig, PhoenixKernel};
use phoenix_workspace::ContentHash;
use std::path::{Path, PathBuf};
use std::sync::Arc;

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
        Some("compare-semantics") if args.len() == 5 => compare_semantics(
            Path::new(&args[1]),
            Path::new(&args[2]),
            Path::new(&args[3]),
            Path::new(&args[4]),
        ),
        _ => bail!(
            "usage: phoenix-analysis-proof seed-and-publish <workspace> <publication-root> \
             <document> <source-document-id> <bridge> <ner-model-root> <nli-model-root>\n\
             or: phoenix-analysis-proof verify <workspace> <publication-root> <blake3>\n\
             or: phoenix-analysis-proof compare-semantics \
             <analysis-a> <nli-a> <analysis-b> <nli-b>"
        ),
    }
}

fn seed_and_publish(
    workspace_path: &Path,
    publication_root: &Path,
    document_path: &Path,
    source_document_id: String,
    bridge: PathBuf,
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
        &LegacyAnalysisAdapterConfig {
            executable: bridge,
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
    kernel.shutdown()?;
    Ok(())
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
