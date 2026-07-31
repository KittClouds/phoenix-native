mod artifact;
mod baseline;
mod model;
mod native_recall;
mod prepare;
mod qps;
mod qps_qualification;
mod verify;

use std::collections::BTreeMap;
use std::env;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Serialize;

fn main() {
    if let Err(error) = run() {
        eprintln!("phoenix-memory-lock: {error:#}");
        std::process::exit(2);
    }
}

fn run() -> Result<()> {
    let mut raw = env::args().skip(1);
    let command = raw.next().context("missing command")?;
    let args = ParsedArgs::parse(raw)?;
    let manifest_path = args.required_path("--manifest")?;
    let (manifest, verification) = verify::load_and_verify(&manifest_path)?;
    match command.as_str() {
        "verify" => print_json(&verification),
        "prepare" => {
            let receipt = prepare::prepare(
                &manifest,
                args.required("--variant")?,
                &args.required_path("--source")?,
                &args.required_path("--workload")?,
                &args.required_path("--gold")?,
            )?;
            print_json(&receipt)
        }
        "baseline" => {
            let top_k = args
                .optional("--top-k")
                .unwrap_or("10")
                .parse::<usize>()
                .context("--top-k must be an integer")?;
            let receipt = baseline::run(
                &manifest,
                &args.required_path("--workload")?,
                &args.required_path("--output")?,
                top_k,
            )?;
            print_json(&receipt)
        }
        "native-recall" => {
            let top_k = args
                .optional("--top-k")
                .unwrap_or("10")
                .parse::<usize>()
                .context("--top-k must be an integer")?;
            let receipt = native_recall::run(
                &manifest,
                &args.required_path("--workload")?,
                &args.required_path("--output")?,
                top_k,
            )?;
            print_json(&receipt)
        }
        "qps-shadow" => {
            let top_k = args
                .optional("--top-k")
                .unwrap_or("10")
                .parse::<usize>()
                .context("--top-k must be an integer")?;
            let repetitions = args
                .optional("--repetitions")
                .unwrap_or("32")
                .parse::<usize>()
                .context("--repetitions must be an integer")?;
            let profile =
                qps::QpsAblationProfile::parse(args.optional("--profile").unwrap_or("full"))?;
            let receipt = qps::run(
                &manifest,
                &args.required_path("--workload")?,
                &args.required_path("--output")?,
                top_k,
                repetitions,
                profile,
            )?;
            print_json(&receipt)
        }
        "evaluate" => {
            let receipt = baseline::evaluate(
                &manifest,
                &args.required_path("--gold")?,
                &args.required_path("--retrieval")?,
            )?;
            print_json(&receipt)
        }
        "qps-qualify" => {
            let repetitions = args
                .optional("--repetitions")
                .unwrap_or("64")
                .parse::<usize>()
                .context("--repetitions must be an integer")?;
            let receipt = qps_qualification::run(&args.required_path("--suite")?, repetitions)?;
            print_json(&receipt)
        }
        "qps-concurrent" => {
            let workers = parse_worker_counts(args.optional("--workers").unwrap_or("1,2,4,8,16"))?;
            let operations_per_worker = args
                .optional("--operations-per-worker")
                .unwrap_or("8192")
                .parse::<usize>()
                .context("--operations-per-worker must be an integer")?;
            let queue_batches_per_worker = args
                .optional("--queue-batches-per-worker")
                .unwrap_or("1")
                .parse::<usize>()
                .context("--queue-batches-per-worker must be an integer")?;
            let batch_size = args
                .optional("--batch-size")
                .unwrap_or("16")
                .parse::<usize>()
                .context("--batch-size must be an integer")?;
            let receipt = qps_qualification::run_concurrent(
                &args.required_path("--suite")?,
                &workers,
                operations_per_worker,
                queue_batches_per_worker,
                batch_size,
            )?;
            print_json(&receipt)
        }
        "qps-concurrent-workload" => {
            let workers = parse_worker_counts(args.optional("--workers").unwrap_or("1,2,4,8,16"))?;
            let operations_per_worker = args
                .optional("--operations-per-worker")
                .unwrap_or("4096")
                .parse::<usize>()
                .context("--operations-per-worker must be an integer")?;
            let queue_batches_per_worker = args
                .optional("--queue-batches-per-worker")
                .unwrap_or("1")
                .parse::<usize>()
                .context("--queue-batches-per-worker must be an integer")?;
            let batch_size = args
                .optional("--batch-size")
                .unwrap_or("1")
                .parse::<usize>()
                .context("--batch-size must be an integer")?;
            let receipt = qps_qualification::run_workload_concurrent(
                &manifest,
                &args.required_path("--workload")?,
                &workers,
                operations_per_worker,
                queue_batches_per_worker,
                batch_size,
            )?;
            print_json(&receipt)
        }
        _ => bail!(
            "unknown command {command:?}; expected verify, prepare, baseline, native-recall, \
             qps-shadow, qps-qualify, qps-concurrent, qps-concurrent-workload, \
             or evaluate"
        ),
    }
}

fn parse_worker_counts(value: &str) -> Result<Vec<usize>> {
    let workers = value
        .split(',')
        .map(|part| {
            part.trim()
                .parse::<usize>()
                .with_context(|| format!("invalid worker count {part:?}"))
        })
        .collect::<Result<Vec<_>>>()?;
    if workers.is_empty()
        || workers.iter().any(|workers| *workers == 0 || *workers > 64)
        || workers.windows(2).any(|pair| pair[0] >= pair[1])
    {
        bail!("--workers must be a strictly increasing list in 1..=64");
    }
    Ok(workers)
}

fn print_json<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

struct ParsedArgs {
    values: BTreeMap<String, String>,
}

impl ParsedArgs {
    fn parse(arguments: impl Iterator<Item = String>) -> Result<Self> {
        let mut arguments = arguments;
        let mut values = BTreeMap::new();
        while let Some(key) = arguments.next() {
            if !key.starts_with("--") {
                bail!("expected option, found {key:?}");
            }
            let value = arguments
                .next()
                .with_context(|| format!("missing value for {key}"))?;
            if values.insert(key.clone(), value).is_some() {
                bail!("duplicate option {key}");
            }
        }
        Ok(Self { values })
    }

    fn required(&self, key: &str) -> Result<&str> {
        self.optional(key)
            .with_context(|| format!("missing required option {key}"))
    }

    fn optional(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    fn required_path(&self, key: &str) -> Result<PathBuf> {
        Ok(Path::new(self.required(key)?).to_path_buf())
    }
}
