//! R3-H scientific sufficiency gate.
//!
//! This gate consumes validator receipts only. It does not inspect packet
//! bodies, private ledgers, qrels, or pair contents. A dataset earns scientific
//! eligibility only when its frozen minimum number of query groups contains
//! both an explicit 2 and an explicit 0.

use std::env;
use std::fs;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

const CONTRACT: &str = "phoenix.qps.r3h-scientific-sufficiency/v1";

#[derive(Debug, Deserialize)]
struct AuthorityReceipt {
    status: String,
    packets: usize,
    labels_two: usize,
    labels_one: usize,
    labels_zero: usize,
    labels_unknown: usize,
    labels_unfilled: usize,
    queries: usize,
    queries_with_two_and_zero: usize,
    authoritative_pairs: usize,
}

#[derive(Debug, Serialize)]
struct DatasetGate {
    dataset: String,
    receipt_file: String,
    validator_status: String,
    packets: usize,
    labels_two: usize,
    labels_one: usize,
    labels_zero: usize,
    labels_unknown: usize,
    labels_unfilled: usize,
    queries: usize,
    queries_with_two_and_zero: usize,
    authoritative_pairs: usize,
    scientific_status: &'static str,
    eligible: bool,
}

#[derive(Debug, Serialize)]
struct GateReceipt {
    contract: &'static str,
    minimum_authoritative_queries: usize,
    status: &'static str,
    eligible_datasets: Vec<String>,
    datasets: Vec<DatasetGate>,
}

fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    let minimum = args
        .next()
        .context("usage: qps_v3_r3_h_sufficiency <min-query-groups> <output-json> <dataset> <receipt> ...")?
        .parse::<usize>()
        .context("minimum query groups must be an integer")?;
    if minimum == 0 {
        bail!("minimum query groups must be greater than zero");
    }
    let output = PathBuf::from(args.next().context("missing output json")?);
    let remaining = args.collect::<Vec<_>>();
    if remaining.is_empty() || remaining.len() % 2 != 0 {
        bail!("expected one or more <dataset> <authority-receipt> pairs");
    }

    let mut datasets = Vec::with_capacity(remaining.len() / 2);
    for pair in remaining.chunks_exact(2) {
        let dataset = pair[0].clone();
        let receipt_file = PathBuf::from(&pair[1]);
        let bytes = fs::read(&receipt_file)
            .with_context(|| format!("read authority receipt {}", receipt_file.display()))?;
        let receipt: AuthorityReceipt = serde_json::from_slice(&bytes)
            .with_context(|| format!("decode authority receipt {}", receipt_file.display()))?;
        let (scientific_status, eligible) = classify(&receipt, minimum);
        datasets.push(DatasetGate {
            dataset,
            receipt_file: receipt_file.display().to_string(),
            validator_status: receipt.status,
            packets: receipt.packets,
            labels_two: receipt.labels_two,
            labels_one: receipt.labels_one,
            labels_zero: receipt.labels_zero,
            labels_unknown: receipt.labels_unknown,
            labels_unfilled: receipt.labels_unfilled,
            queries: receipt.queries,
            queries_with_two_and_zero: receipt.queries_with_two_and_zero,
            authoritative_pairs: receipt.authoritative_pairs,
            scientific_status,
            eligible,
        });
    }

    let eligible_datasets = datasets
        .iter()
        .filter(|dataset| dataset.eligible)
        .map(|dataset| dataset.dataset.clone())
        .collect::<Vec<_>>();
    let status = if datasets
        .iter()
        .any(|dataset| dataset.validator_status == "awaiting_judgments")
    {
        "awaiting_judgments"
    } else if eligible_datasets.len() == datasets.len() {
        "all_sufficient"
    } else if eligible_datasets.is_empty() {
        "underpowered_query_count"
    } else {
        "mixed"
    };
    let receipt = GateReceipt {
        contract: CONTRACT,
        minimum_authoritative_queries: minimum,
        status,
        eligible_datasets,
        datasets,
    };
    fs::write(&output, serde_json::to_vec_pretty(&receipt)?)?;
    println!("{}", serde_json::to_string_pretty(&receipt)?);
    Ok(())
}

fn classify(receipt: &AuthorityReceipt, minimum: usize) -> (&'static str, bool) {
    if receipt.status == "awaiting_judgments" || receipt.labels_unfilled > 0 {
        ("awaiting_judgments", false)
    } else if receipt.queries_with_two_and_zero >= minimum {
        ("sufficient", true)
    } else {
        ("underpowered_query_count", false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn receipt(status: &str, unfilled: usize, groups: usize) -> AuthorityReceipt {
        AuthorityReceipt {
            status: status.into(),
            packets: 96,
            labels_two: 0,
            labels_one: 0,
            labels_zero: 0,
            labels_unknown: 0,
            labels_unfilled: unfilled,
            queries: 24,
            queries_with_two_and_zero: groups,
            authoritative_pairs: groups,
        }
    }

    #[test]
    fn review_in_progress_is_not_eligible() {
        assert_eq!(
            classify(&receipt("awaiting_judgments", 96, 24), 8),
            ("awaiting_judgments", false)
        );
    }

    #[test]
    fn query_group_threshold_is_independent_of_pair_count() {
        assert_eq!(
            classify(&receipt("authority_available", 0, 7), 8),
            ("underpowered_query_count", false)
        );
        assert_eq!(
            classify(&receipt("authority_available", 0, 8), 8),
            ("sufficient", true)
        );
    }
}
