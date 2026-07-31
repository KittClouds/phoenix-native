use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Instant;

use anyhow::{anyhow, bail, Context, Result};
use async_channel::Receiver;
use phoenix_lexical_qps::{QpsIndex, SearchHit, SearchScratch, MAXIMUM_QUERY_GROUPS};
use serde::Serialize;

use super::concurrency::{dispatch_phase, private_usage_bytes, queries_per_second, QueryBatch};
use super::{binary_identity, elapsed_nanos, hit_documents, ranking_matches, LatencyPercentiles};
use crate::artifact::read_artifact;
use crate::baseline::verify_binding;
use crate::model::{FreezeManifest, WorkloadArtifact, WORKLOAD_CONTRACT, WORKLOAD_MAGIC};
use crate::qps::{build_qps, QpsAblationProfile};

const CONTRACT: &str = "phoenix.memory.qps-workload-concurrency/v1";
const AUTHORITY: &str = "qps-only-sharded-bounded-workers-no-bm25-arm";
const PHASES: usize = 2;
const ENGINE_P99_BUDGET_NANOS: u64 = 1_000_000;
const END_TO_END_P99_BUDGET_NANOS: u64 = 10_000_000;
const STEADY_PRIVATE_GROWTH_BUDGET_BYTES: usize = 1 << 20;

pub fn run(
    manifest: &FreezeManifest,
    workload_path: &Path,
    worker_counts: &[usize],
    operations_per_worker: usize,
    queue_batches_per_worker: usize,
    batch_size: usize,
) -> Result<WorkloadConcurrencyReceipt> {
    validate_configuration(
        worker_counts,
        operations_per_worker,
        queue_batches_per_worker,
        batch_size,
    )?;
    let workload: WorkloadArtifact = read_artifact(workload_path, WORKLOAD_MAGIC)?;
    if workload.contract != WORKLOAD_CONTRACT {
        bail!("unsupported workload contract {}", workload.contract);
    }
    verify_binding(manifest, &workload.source)?;
    let build_started = Instant::now();
    let (plans, index_stats) = build_plans(&workload)?;
    let build_nanos = elapsed_nanos(build_started);
    let plans = Arc::new(plans);
    let logical_processors = thread::available_parallelism().map_or(1, usize::from);
    let mut sweeps = Vec::with_capacity(worker_counts.len());

    for &workers in worker_counts {
        sweeps.push(run_sweep(
            Arc::clone(&plans),
            workers,
            operations_per_worker,
            queue_batches_per_worker,
            batch_size,
        )?);
    }
    let baseline = sweeps
        .first()
        .map(|sweep| sweep.steady_queries_per_second)
        .unwrap_or(0.0);
    for sweep in &mut sweeps {
        sweep.speedup_vs_first = if baseline > 0.0 {
            sweep.steady_queries_per_second / baseline
        } else {
            0.0
        };
        sweep.parallel_efficiency = sweep.speedup_vs_first / sweep.workers as f64;
    }
    let best = sweeps
        .iter()
        .max_by(|left, right| {
            left.steady_queries_per_second
                .total_cmp(&right.steady_queries_per_second)
        })
        .context("workload concurrency sweep produced no results")?;
    let recommended_workers = best.workers;
    let peak_queries_per_second = best.steady_queries_per_second;
    let peak_speedup_vs_first = best.speedup_vs_first;
    let all_absolute_gates_pass = sweeps.iter().all(|sweep| sweep.gates.all_pass());
    let scaling_gate = logical_processors < 2 || peak_speedup_vs_first >= 1.5;
    let qualified = all_absolute_gates_pass && scaling_gate;

    Ok(WorkloadConcurrencyReceipt {
        contract: CONTRACT,
        authority: AUTHORITY,
        workload_path: workload_path.display().to_string(),
        freeze_id: workload.source.freeze_id,
        source_sha256: workload.source.sha256,
        binary: binary_identity()?,
        logical_processors,
        cases: plans.len(),
        build_nanos,
        indexes: index_stats,
        operations_per_worker_per_phase: operations_per_worker,
        phases: PHASES,
        batch_size,
        queue_batches_per_worker,
        sweeps,
        recommended_workers,
        peak_queries_per_second,
        peak_speedup_vs_first,
        all_absolute_gates_pass,
        peak_speedup_at_least_1_5x: scaling_gate,
        qualified_for_bounded_multi_worker_use: qualified,
    })
}

fn validate_configuration(
    worker_counts: &[usize],
    operations_per_worker: usize,
    queue_batches_per_worker: usize,
    batch_size: usize,
) -> Result<()> {
    if worker_counts.is_empty()
        || worker_counts
            .iter()
            .any(|workers| *workers == 0 || *workers > 64)
        || worker_counts.windows(2).any(|pair| pair[0] >= pair[1])
    {
        bail!("worker counts must be strictly increasing in 1..=64");
    }
    if !(1..=64).contains(&batch_size) {
        bail!("batch size must be in 1..=64");
    }
    if !(batch_size..=1_048_576).contains(&operations_per_worker) {
        bail!("operations per worker must be in {batch_size}..=1048576");
    }
    if !(1..=1_024).contains(&queue_batches_per_worker) {
        bail!("queue batches per worker must be in 1..=1024");
    }
    Ok(())
}

fn build_plans(workload: &WorkloadArtifact) -> Result<(Vec<WorkloadPlan>, IndexSetReceipt)> {
    let mut plans = Vec::with_capacity(workload.cases.len());
    let mut indexes = IndexSetReceipt::default();
    for case in &workload.cases {
        let documents = case
            .sessions
            .iter()
            .map(|session| {
                session
                    .turns
                    .iter()
                    .map(|turn| turn.content.as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .collect::<Vec<_>>();
        let index = build_qps(&documents, QpsAblationProfile::Full)?;
        let stats = index.stats();
        indexes.documents += stats.documents;
        indexes.terms += stats.terms;
        indexes.posting_rows += stats.posting_rows;
        indexes.positions += stats.positions;
        indexes.estimated_bytes += stats.estimated_bytes;
        indexes.maximum_documents_per_index =
            indexes.maximum_documents_per_index.max(stats.documents);
        let mut scratch =
            SearchScratch::with_document_capacity(documents.len(), MAXIMUM_QUERY_GROUPS);
        let mut hits = Vec::<SearchHit>::with_capacity(10);
        index.search_into(&case.question, 10, &mut scratch, &mut hits)?;
        plans.push(WorkloadPlan {
            index,
            query: case.question.clone(),
            expected: hit_documents(&hits).into_boxed_slice(),
        });
    }
    indexes.indexes = plans.len();
    Ok((plans, indexes))
}

fn run_sweep(
    plans: Arc<Vec<WorkloadPlan>>,
    workers: usize,
    operations_per_worker: usize,
    queue_batches_per_worker: usize,
    batch_size: usize,
) -> Result<WorkloadWorkerSweep> {
    let channels = (0..workers)
        .map(|_| async_channel::bounded::<QueryBatch>(queue_batches_per_worker))
        .collect::<Vec<_>>();
    let senders = channels
        .iter()
        .map(|(sender, _)| sender.clone())
        .collect::<Vec<_>>();
    let receivers = channels
        .into_iter()
        .map(|(_, receiver)| receiver)
        .collect::<Vec<_>>();
    let completed = [AtomicUsize::new(0), AtomicUsize::new(0)];
    let ready = Barrier::new(workers + 1);
    let mut phase_wall_nanos = [0_u64; PHASES];
    let mut phase_queue_high_water = [0_usize; PHASES];
    let mut private_usage = [0_usize; PHASES + 1];

    let results = thread::scope(|scope| -> Result<Vec<WorkloadWorkerResult>> {
        let handles = receivers
            .into_iter()
            .map(|receiver| {
                let plans = Arc::clone(&plans);
                let completed = &completed;
                let ready = &ready;
                scope.spawn(move || {
                    worker_loop(&plans, receiver, completed, ready, operations_per_worker)
                })
            })
            .collect::<Vec<_>>();
        ready.wait();
        private_usage[0] = private_usage_bytes()?;
        for phase in 0..PHASES {
            let started = Instant::now();
            dispatch_phase(
                &senders,
                phase,
                operations_per_worker,
                plans.len(),
                batch_size,
                &mut phase_queue_high_water[phase],
            )?;
            let expected = workers.saturating_mul(operations_per_worker);
            while completed[phase].load(Ordering::Acquire) < expected {
                thread::yield_now();
            }
            phase_wall_nanos[phase] = elapsed_nanos(started);
            private_usage[phase + 1] = private_usage_bytes()?;
        }
        drop(senders);
        handles
            .into_iter()
            .map(|handle| {
                handle
                    .join()
                    .map_err(|_| anyhow!("QPS workload worker panicked"))?
            })
            .collect()
    })?;

    let mut steady = WorkloadSamples::default();
    let mut scratch_growths = 0_u64;
    let mut determinism_failures = 0_u64;
    let mut worker_searches = Vec::with_capacity(workers);
    for mut result in results {
        steady.append(std::mem::take(&mut result.phases[1]));
        scratch_growths += result.scratch_growths;
        determinism_failures += result.determinism_failures;
        worker_searches.push(result.searches);
    }
    let total = workers.saturating_mul(operations_per_worker);
    let steady_private_growth = private_usage[2].saturating_sub(private_usage[1]);
    let latency = steady.finish();
    let gates = WorkloadSweepGates {
        engine_p99_at_most_one_millisecond: latency.engine.p99 <= ENGINE_P99_BUDGET_NANOS,
        end_to_end_p99_at_most_ten_milliseconds: latency.end_to_end.p99
            <= END_TO_END_P99_BUDGET_NANOS,
        zero_scratch_capacity_growth: scratch_growths == 0,
        deterministic_rankings: determinism_failures == 0,
        bounded_queues: phase_queue_high_water
            .iter()
            .all(|high_water| *high_water <= workers.saturating_mul(queue_batches_per_worker)),
        every_worker_received_exact_share: worker_searches
            .iter()
            .all(|searches| *searches == operations_per_worker.saturating_mul(PHASES)),
        stable_private_memory: steady_private_growth <= STEADY_PRIVATE_GROWTH_BUDGET_BYTES,
    };

    Ok(WorkloadWorkerSweep {
        workers,
        total_queries_per_phase: total,
        first_phase_wall_nanos: phase_wall_nanos[0],
        steady_phase_wall_nanos: phase_wall_nanos[1],
        first_queries_per_second: queries_per_second(total, phase_wall_nanos[0]),
        steady_queries_per_second: queries_per_second(total, phase_wall_nanos[1]),
        speedup_vs_first: 0.0,
        parallel_efficiency: 0.0,
        steady_latency_nanos: latency,
        queue_high_water_batches: phase_queue_high_water[1],
        queue_capacity_batches: workers.saturating_mul(queue_batches_per_worker),
        scratch_capacity_growths: scratch_growths,
        determinism_failures,
        worker_searches,
        private_usage_bytes: WorkloadPrivateUsage {
            after_worker_warmup: private_usage[0],
            after_first_phase: private_usage[1],
            after_steady_phase: private_usage[2],
            steady_growth: steady_private_growth,
        },
        gates,
    })
}

fn worker_loop(
    plans: &[WorkloadPlan],
    receiver: Receiver<QueryBatch>,
    completed: &[AtomicUsize; PHASES],
    ready: &Barrier,
    operations_per_worker: usize,
) -> Result<WorkloadWorkerResult> {
    let maximum_documents = plans
        .iter()
        .map(|plan| plan.index.stats().documents)
        .max()
        .unwrap_or(0);
    let mut scratch =
        SearchScratch::with_document_capacity(maximum_documents, MAXIMUM_QUERY_GROUPS);
    let mut hits = Vec::<SearchHit>::with_capacity(10);
    for plan in plans {
        plan.index
            .search_into(&plan.query, 10, &mut scratch, &mut hits)?;
    }
    let mut result = WorkloadWorkerResult::new(operations_per_worker);
    ready.wait();
    while let Ok(batch) = receiver.recv_blocking() {
        let phase = usize::from(batch.phase);
        let len = usize::from(batch.len);
        for offset in 0..len {
            let plan = &plans[(batch.sequence_start + offset) % plans.len()];
            let queue_nanos = elapsed_nanos(batch.enqueued);
            let started = Instant::now();
            let receipt = plan
                .index
                .search_into(&plan.query, 10, &mut scratch, &mut hits)?;
            let engine_nanos = elapsed_nanos(started);
            result.phases[phase].engine.push(engine_nanos);
            result.phases[phase].queue.push(queue_nanos);
            result.phases[phase]
                .end_to_end
                .push(elapsed_nanos(batch.enqueued));
            result.scratch_growths += u64::from(receipt.allocations_grew);
            result.determinism_failures +=
                u64::from(!ranking_matches(&hits, plan.expected.as_ref()));
            result.searches += 1;
        }
        completed[phase].fetch_add(len, Ordering::Release);
    }
    Ok(result)
}

struct WorkloadPlan {
    index: QpsIndex,
    query: String,
    expected: Box<[u64]>,
}

struct WorkloadWorkerResult {
    phases: [WorkloadSamples; PHASES],
    scratch_growths: u64,
    determinism_failures: u64,
    searches: usize,
}

impl WorkloadWorkerResult {
    fn new(capacity: usize) -> Self {
        Self {
            phases: std::array::from_fn(|_| WorkloadSamples::with_capacity(capacity)),
            scratch_growths: 0,
            determinism_failures: 0,
            searches: 0,
        }
    }
}

#[derive(Default)]
struct WorkloadSamples {
    engine: Vec<u64>,
    queue: Vec<u64>,
    end_to_end: Vec<u64>,
}

impl WorkloadSamples {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            engine: Vec::with_capacity(capacity),
            queue: Vec::with_capacity(capacity),
            end_to_end: Vec::with_capacity(capacity),
        }
    }

    fn append(&mut self, mut other: Self) {
        self.engine.append(&mut other.engine);
        self.queue.append(&mut other.queue);
        self.end_to_end.append(&mut other.end_to_end);
    }

    fn finish(mut self) -> WorkloadLatency {
        WorkloadLatency {
            engine: LatencyPercentiles::from_samples(&mut self.engine),
            queue: LatencyPercentiles::from_samples(&mut self.queue),
            end_to_end: LatencyPercentiles::from_samples(&mut self.end_to_end),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct WorkloadConcurrencyReceipt {
    contract: &'static str,
    authority: &'static str,
    workload_path: String,
    freeze_id: String,
    source_sha256: String,
    binary: super::BinaryIdentity,
    logical_processors: usize,
    cases: usize,
    build_nanos: u64,
    indexes: IndexSetReceipt,
    operations_per_worker_per_phase: usize,
    phases: usize,
    batch_size: usize,
    queue_batches_per_worker: usize,
    sweeps: Vec<WorkloadWorkerSweep>,
    recommended_workers: usize,
    peak_queries_per_second: f64,
    peak_speedup_vs_first: f64,
    all_absolute_gates_pass: bool,
    peak_speedup_at_least_1_5x: bool,
    qualified_for_bounded_multi_worker_use: bool,
}

#[derive(Debug, Default, Serialize)]
struct IndexSetReceipt {
    indexes: usize,
    documents: usize,
    terms: usize,
    posting_rows: usize,
    positions: usize,
    estimated_bytes: usize,
    maximum_documents_per_index: usize,
}

#[derive(Debug, Serialize)]
struct WorkloadWorkerSweep {
    workers: usize,
    total_queries_per_phase: usize,
    first_phase_wall_nanos: u64,
    steady_phase_wall_nanos: u64,
    first_queries_per_second: f64,
    steady_queries_per_second: f64,
    speedup_vs_first: f64,
    parallel_efficiency: f64,
    steady_latency_nanos: WorkloadLatency,
    queue_high_water_batches: usize,
    queue_capacity_batches: usize,
    scratch_capacity_growths: u64,
    determinism_failures: u64,
    worker_searches: Vec<usize>,
    private_usage_bytes: WorkloadPrivateUsage,
    gates: WorkloadSweepGates,
}

#[derive(Debug, Serialize)]
struct WorkloadLatency {
    engine: LatencyPercentiles,
    queue: LatencyPercentiles,
    end_to_end: LatencyPercentiles,
}

#[derive(Debug, Serialize)]
struct WorkloadPrivateUsage {
    after_worker_warmup: usize,
    after_first_phase: usize,
    after_steady_phase: usize,
    steady_growth: usize,
}

#[derive(Debug, Serialize)]
struct WorkloadSweepGates {
    engine_p99_at_most_one_millisecond: bool,
    end_to_end_p99_at_most_ten_milliseconds: bool,
    zero_scratch_capacity_growth: bool,
    deterministic_rankings: bool,
    bounded_queues: bool,
    every_worker_received_exact_share: bool,
    stable_private_memory: bool,
}

impl WorkloadSweepGates {
    fn all_pass(&self) -> bool {
        self.engine_p99_at_most_one_millisecond
            && self.end_to_end_p99_at_most_ten_milliseconds
            && self.zero_scratch_capacity_growth
            && self.deterministic_rankings
            && self.bounded_queues
            && self.every_worker_received_exact_share
            && self.stable_private_memory
    }
}
