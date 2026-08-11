#[path = "independent/artifact.rs"]
mod artifact;
#[path = "independent/build.rs"]
mod build;
#[path = "independent/finalize.rs"]
mod finalize;
#[path = "independent/fuzzy.rs"]
mod fuzzy;
#[path = "independent/merge.rs"]
mod merge;
#[path = "independent/partition.rs"]
mod partition;
#[path = "independent/review.rs"]
mod review;
#[path = "independent/review_batch.rs"]
mod review_batch;
#[path = "independent/review_bundle.rs"]
mod review_bundle;
#[path = "independent/source.rs"]
mod source;

use std::env;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};

use build::{BuildInputs, BuildOutputs};

fn main() {
    if let Err(error) = run() {
        eprintln!("phoenix-v3-independent-data: {error:#}");
        std::process::exit(2);
    }
}

fn run() -> Result<()> {
    let mut raw = env::args().skip(1);
    let command = raw.next().context("missing command")?;
    let args = Args::parse(raw)?;
    match command.as_str() {
        "generate" => {
            let publication = build::generate(
                &BuildInputs {
                    workspace_key: args.required_path("--workspace-key")?,
                    phase_3: args.required_path("--phase-3")?,
                    locomo: args.required_path("--locomo")?,
                    scifact: args.required_path("--scifact")?,
                    nfcorpus: args.required_path("--nfcorpus")?,
                },
                &BuildOutputs {
                    ledger: args.required_path("--ledger-output")?,
                    graded: args.required_path("--graded-output")?,
                    review: args.required_path("--review-output")?,
                    receipt: args.required_path("--receipt-output")?,
                },
            )?;
            println!("{}", serde_json::to_string_pretty(&publication)?);
        }
        "apply-reviews" => {
            let publication = review::apply(
                &args.required_path("--ledger")?,
                &args.required_path("--decisions")?,
                &args.required_path("--ledger-output")?,
                &args.required_path("--receipt-output")?,
            )?;
            println!("{}", serde_json::to_string_pretty(&publication)?);
        }
        "finalize-audited-decisions" => {
            let publication = finalize::finalize(
                &args.required_path("--decision-cut-a")?,
                &args.required_path("--decision-cut-b")?,
                &args.required_path("--audit-a")?,
                &args.required_path("--audit-b")?,
                &args.required_path("--audit-c")?,
                &args.required_path("--output")?,
                &args.required_path("--receipt-output")?,
            )?;
            println!("{}", serde_json::to_string_pretty(&publication)?);
        }
        "merge-agent-decisions" => {
            let publication = merge::merge(
                &args.required_path("--cuts-root")?,
                &args.required_path("--packet")?,
                &args.required_path("--output")?,
                &args.required_path("--receipt-output")?,
            )?;
            println!("{}", serde_json::to_string_pretty(&publication)?);
        }
        "prepare-review-batch" => {
            let publication = review_batch::prepare(
                &args.required_path("--review-packet")?,
                &args.required_path("--locomo")?,
                args.optional_path("--exclude-batch").as_deref(),
                args.required("--limit")?
                    .parse::<usize>()
                    .context("--limit must be a positive integer")?,
                &args.required_path("--output")?,
            )?;
            println!("{}", serde_json::to_string_pretty(&publication)?);
        }
        "audit-review-batch" => {
            let publication = review_batch::audit(
                &args.required_path("--batch")?,
                &args.required_path("--output")?,
            )?;
            println!("{}", serde_json::to_string_pretty(&publication)?);
        }
        "prepare-review-bundles" => {
            let publication = review_bundle::prepare(
                &args.required_path("--review-packet")?,
                &args.required_path("--locomo")?,
                args.optional_path("--exclude-batch").as_deref(),
                &args.required_path("--phase-5")?,
                args.required("--limit")?
                    .parse::<usize>()
                    .context("--limit must be a positive integer")?,
                &args.required_path("--output")?,
            )?;
            println!("{}", serde_json::to_string_pretty(&publication)?);
        }
        "prepare-confirmation-bundles" => {
            let publication = review_bundle::prepare_confirmation(
                &args.required_path("--review-packet")?,
                &args.required_path("--locomo")?,
                &args.required_path("--exclude-batch")?,
                &args.required_path("--phase-5")?,
                args.required("--limit")?
                    .parse::<usize>()
                    .context("--limit must be a positive integer")?,
                &args.required_path("--output")?,
            )?;
            println!("{}", serde_json::to_string_pretty(&publication)?);
        }
        "audit-review-bundles" => {
            let publication = review_bundle::audit(
                &args.required_path("--batch")?,
                &args.required_path("--output")?,
            )?;
            println!("{}", serde_json::to_string_pretty(&publication)?);
        }
        "audit-locomo-partitions" => {
            let dataset = source::load_locomo(&args.required_path("--locomo")?, 0..8)?;
            let partitions = partition::MiningPartitions::new(&dataset);
            println!("{}", serde_json::to_string_pretty(&partitions.audit())?);
        }
        _ => bail!(
            "expected `generate`, `prepare-review-batch`, `audit-review-batch`, `prepare-review-bundles`, `prepare-confirmation-bundles`, `audit-review-bundles`, `apply-reviews`, `finalize-audited-decisions`, `merge-agent-decisions`, or `audit-locomo-partitions`"
        ),
    }
    Ok(())
}

struct Args(Vec<(String, String)>);

impl Args {
    fn parse(raw: impl Iterator<Item = String>) -> Result<Self> {
        let values = raw.collect::<Vec<_>>();
        if values.len() % 2 != 0 {
            bail!("arguments must be --name value pairs");
        }
        let mut pairs = Vec::with_capacity(values.len() / 2);
        for pair in values.chunks_exact(2) {
            if !pair[0].starts_with("--") {
                bail!("unexpected positional argument {}", pair[0]);
            }
            if pairs.iter().any(|(name, _)| name == &pair[0]) {
                bail!("duplicate argument {}", pair[0]);
            }
            pairs.push((pair[0].clone(), pair[1].clone()));
        }
        Ok(Self(pairs))
    }

    fn required_path(&self, name: &str) -> Result<PathBuf> {
        self.0
            .iter()
            .find_map(|(candidate, value)| (candidate == name).then(|| PathBuf::from(value)))
            .with_context(|| format!("missing {name}"))
    }

    fn required(&self, name: &str) -> Result<&str> {
        self.0
            .iter()
            .find_map(|(candidate, value)| (candidate == name).then_some(value.as_str()))
            .with_context(|| format!("missing {name}"))
    }

    fn optional_path(&self, name: &str) -> Option<PathBuf> {
        self.0
            .iter()
            .find_map(|(candidate, value)| (candidate == name).then(|| PathBuf::from(value)))
    }
}
