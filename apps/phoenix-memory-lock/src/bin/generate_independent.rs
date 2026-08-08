#[path = "independent/artifact.rs"]
mod artifact;
#[path = "independent/build.rs"]
mod build;
#[path = "independent/curate.rs"]
mod curate;
#[path = "independent/fuzzy.rs"]
mod fuzzy;
#[path = "independent/partition.rs"]
mod partition;
#[path = "independent/review.rs"]
mod review;
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
        "curate-benchmark" => {
            let publication = curate::curate(
                &args.required_path("--ledger")?,
                &args.required_path("--review-packet")?,
                &args.required_path("--generation-receipt")?,
                args.required("--authorization-context")?,
                args.required("--reviewer-identity")?,
                args.required("--reviewed-at")?
                    .parse::<u64>()
                    .context("--reviewed-at must be a Unix timestamp")?,
                &args.required_path("--decisions-output")?,
                &args.required_path("--receipt-output")?,
            )?;
            println!("{}", serde_json::to_string_pretty(&publication)?);
        }
        "audit-locomo-partitions" => {
            let dataset = source::load_locomo(&args.required_path("--locomo")?, 0..8)?;
            let partitions = partition::MiningPartitions::new(&dataset);
            println!("{}", serde_json::to_string_pretty(&partitions.audit())?);
        }
        _ => bail!(
            "expected `generate`, `curate-benchmark`, `apply-reviews`, or `audit-locomo-partitions`"
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
}
