//! Read-only R3 authority-contract audit for the rarity microscope.
//!
//! The R3 scientific arms are intentionally not run unless qrels contain
//! explicit non-relevant judgments. BEIR distributions normally provide
//! positive-only qrels; unjudged rows are never promoted to update authority.

use std::collections::HashMap;
use std::env;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Default, Serialize)]
struct SplitAudit {
    file: String,
    rows: usize,
    queries: usize,
    positive_rows: usize,
    explicit_negative_rows: usize,
    queries_with_positive: usize,
    queries_with_explicit_negative: usize,
    authoritative_queries: usize,
    authoritative_pairs: usize,
    sha256: String,
}

#[derive(Debug, Serialize)]
struct ArmAudit {
    arm: &'static str,
    status: &'static str,
    reason: &'static str,
}

#[derive(Debug, Serialize)]
struct Receipt {
    contract: &'static str,
    dataset: String,
    status: &'static str,
    negative_policy: &'static str,
    splits: Vec<SplitAudit>,
    arms: Vec<ArmAudit>,
}

fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    let root = PathBuf::from(
        args.next()
            .context("usage: qps_v3_r3_rarity <dataset-root> [output-json]")?,
    );
    let output = args.next().map(PathBuf::from);
    let dataset = root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("beir")
        .to_owned();
    let mut splits = Vec::new();
    for name in ["train.tsv", "dev.tsv", "test.tsv"] {
        let path = root.join("qrels").join(name);
        if path.exists() {
            splits.push(audit_split(&path)?);
        }
    }
    let authoritative_pairs = splits
        .iter()
        .map(|split| split.authoritative_pairs)
        .sum::<usize>();
    let status = if authoritative_pairs > 0 {
        "eligible_for_r3_scientific_arm"
    } else {
        "blocked_no_explicit_nonrelevant_qrels"
    };
    let reason = if authoritative_pairs > 0 {
        "explicit_positive_and_nonrelevant_pairs_are_available"
    } else {
        "all_local_qrels_rows_are_positive; unjudged_documents_cannot_supply_update_authority"
    };
    let arms = [
        "r0_bm25f_anchor",
        "r1_bounded_rarest_matched_term",
        "r2_bounded_rarest_term_times_coverage",
        "r3_bounded_rarity_distribution_gap",
    ]
    .into_iter()
    .map(|arm| ArmAudit {
        arm,
        status: if authoritative_pairs > 0 {
            "ready"
        } else {
            "not_run"
        },
        reason,
    })
    .collect();
    let receipt = Receipt {
        contract: "phoenix.qps.beir-r3-rarity-authority/v1",
        dataset,
        status,
        negative_policy: "only_explicit_score_zero_qrels_are_authoritative_negatives;_unjudged_rows_are_observation_only",
        splits,
        arms,
    };
    let json = serde_json::to_string_pretty(&receipt)?;
    if let Some(path) = output {
        fs::write(&path, json.as_bytes()).with_context(|| format!("write {}", path.display()))?;
    }
    println!("{}", json);
    Ok(())
}

fn audit_split(path: &Path) -> Result<SplitAudit> {
    let mut per_query = HashMap::<String, (usize, usize)>::new();
    let mut receipt = SplitAudit {
        file: path.display().to_string(),
        sha256: sha256_file(path)?,
        ..SplitAudit::default()
    };
    for (line_number, line) in BufReader::new(File::open(path)?).lines().enumerate() {
        let line =
            line.with_context(|| format!("read {} line {}", path.display(), line_number + 1))?;
        if line_number == 0 && line.starts_with("query-id") {
            continue;
        }
        let mut columns = line.split('\t');
        let query = columns.next().context("missing query id")?.to_owned();
        let _document = columns.next().context("missing document id")?;
        let score = columns
            .next()
            .context("missing qrels score")?
            .parse::<i32>()?;
        receipt.rows += 1;
        let counts = per_query.entry(query).or_default();
        if score > 0 {
            receipt.positive_rows += 1;
            counts.0 += 1;
        } else if score == 0 {
            receipt.explicit_negative_rows += 1;
            counts.1 += 1;
        }
    }
    receipt.queries = per_query.len();
    receipt.queries_with_positive = per_query
        .values()
        .filter(|(positive, _)| *positive > 0)
        .count();
    receipt.queries_with_explicit_negative = per_query
        .values()
        .filter(|(_, negative)| *negative > 0)
        .count();
    receipt.authoritative_queries = per_query
        .values()
        .filter(|(positive, negative)| *positive > 0 && *negative > 0)
        .count();
    receipt.authoritative_pairs = per_query
        .values()
        .map(|(positive, negative)| positive * negative)
        .sum();
    Ok(receipt)
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut hasher = Sha256::new();
    hasher.update(fs::read(path).with_context(|| format!("read {}", path.display()))?);
    Ok(format!("{:x}", hasher.finalize()))
}
