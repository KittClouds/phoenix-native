use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Instant;

use anyhow::{anyhow, bail, Context, Result};
use async_channel::{Receiver, Sender};
use phoenix_lexical_qps::{QueryGroup, SearchHit, SearchScratch, MAXIMUM_QUERY_GROUPS};
use serde::Serialize;

use super::{
    binary_identity, build_index, elapsed_nanos, hex_sha256, hit_documents, ranking_matches,
    validate_suite, LatencyPercentiles, PreparedQuery, QualificationSuite,
};

const CONTRACT: &str = "phoenix.memory.qps-concurrency-qualification/v1";
const AUTHORITY: &str = "one-qps-index-sharded-bounded-batch-workers";
const PHASES: usize = 2;
const MAX_BATCH_SIZE: usize = 64;
const ENGINE_P99_BUDGET_NANOS: u64 = 1_000_000;
const END_TO_END_P99_BUDGET_NANOS: u64 = 2_000_000;
const STEADY_PRIVATE_GROWTH_BUDGET_BYTES: usize = 1 << 20;

pub fn run(
    suite_path: &std::path::Path,
    worker_counts: &[usize],
    operations_per_worker: usize,
    queue_batches_per_worker: usize,
    batch_size: usize,
) -> Result<ConcurrencyQualificationReceipt> {
    validate_configuration(
        worker_counts,
        operations_per_worker,
        queue_batches_per_worker,
        batch_size,
    )?;
    let suite_bytes = fs::read(suite_path)
        .with_context(|| format!("read qualification suite {}", suite_path.display()))?;
    let suite: QualificationSuite = serde_json::from_slice(&suite_bytes)
        .with_context(|| format!("decode qualification suite {}", suite_path.display()))?;
    validate_suite(&suite)?;
    let index = Arc::new(build_index(&suite)?);
    assert_send_sync::<phoenix_lexical_qps::QpsIndex>();
    let expected_rankings = expected_rankings(&suite, &index)?;
    let logical_processors = thread::available_parallelism().map_or(1, usize::from);
    let mut sweeps = Vec::with_capacity(worker_counts.len());

    for &workers in worker_counts {
        sweeps.push(run_sweep(
            &suite,
            Arc::clone(&index),
            &expected_rankings,
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
        .context("concurrency sweep produced no results")?;
    let all_absolute_gates_pass = sweeps.iter().all(|sweep| sweep.gates.all_pass());
    let scaling_gate = logical_processors < 2 || best.speedup_vs_first >= 1.5;
    let qualified = all_absolute_gates_pass && scaling_gate;
    let recommended_workers = best.workers;
    let peak_queries_per_second = best.steady_queries_per_second;
    let peak_speedup_vs_first = best.speedup_vs_first;

    Ok(ConcurrencyQualificationReceipt {
        contract: CONTRACT,
        authority: AUTHORITY,
        suite_path: suite_path.display().to_string(),
        suite_bytes: suite_bytes.len(),
        suite_sha256: hex_sha256(&suite_bytes),
        binary: binary_identity()?,
        logical_processors,
        query_shapes: ["ordinary", "phrase", "fuzzy", "no_result"],
        source_kinds: ["document", "conversation"],
        operations_per_worker_per_phase: operations_per_worker,
        phases: PHASES,
        fixed_batch_size: batch_size,
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
    if !(1..=MAX_BATCH_SIZE).contains(&batch_size) {
        bail!("batch size must be in 1..={MAX_BATCH_SIZE}");
    }
    if !(batch_size..=1_048_576).contains(&operations_per_worker) {
        bail!("operations per worker must be in {batch_size}..=1048576");
    }
    if queue_batches_per_worker == 0 || queue_batches_per_worker > 1_024 {
        bail!("queue batches per worker must be in 1..=1024");
    }
    Ok(())
}

fn expected_rankings(
    suite: &QualificationSuite,
    index: &phoenix_lexical_qps::QpsIndex,
) -> Result<Vec<Vec<u64>>> {
    let prepared = suite
        .queries
        .iter()
        .map(PreparedQuery::new)
        .collect::<Vec<_>>();
    let groups = prepared
        .iter()
        .map(PreparedQuery::group_views)
        .collect::<Vec<_>>();
    let mut scratch =
        SearchScratch::with_document_capacity(suite.documents.len(), MAXIMUM_QUERY_GROUPS);
    let mut hits = Vec::<SearchHit>::with_capacity(10);
    prepared
        .iter()
        .zip(&groups)
        .map(|(query, groups)| {
            query.search(groups, index, &mut scratch, &mut hits)?;
            Ok(hit_documents(&hits))
        })
        .collect()
}

fn run_sweep(
    suite: &QualificationSuite,
    index: Arc<phoenix_lexical_qps::QpsIndex>,
    expected_rankings: &[Vec<u64>],
    workers: usize,
    operations_per_worker: usize,
    queue_batches_per_worker: usize,
    batch_size: usize,
) -> Result<WorkerSweepReceipt> {
    let senders_and_receivers = (0..workers)
        .map(|_| async_channel::bounded::<QueryBatch>(queue_batches_per_worker))
        .collect::<Vec<_>>();
    let senders = senders_and_receivers
        .iter()
        .map(|(sender, _)| sender.clone())
        .collect::<Vec<_>>();
    let receivers = senders_and_receivers
        .into_iter()
        .map(|(_, receiver)| receiver)
        .collect::<Vec<_>>();
    let completed = [AtomicUsize::new(0), AtomicUsize::new(0)];
    let ready = Barrier::new(workers + 1);
    let mut phase_wall_nanos = [0_u64; PHASES];
    let mut phase_queue_high_water = [0_usize; PHASES];
    let mut private_usage = [0_usize; PHASES + 1];

    let worker_results = thread::scope(|scope| -> Result<Vec<WorkerResult>> {
        let handles = receivers
            .into_iter()
            .enumerate()
            .map(|(worker, receiver)| {
                let index = Arc::clone(&index);
                let completed = &completed;
                let ready = &ready;
                scope.spawn(move || {
                    worker_loop(
                        worker,
                        suite,
                        &index,
                        expected_rankings,
                        receiver,
                        completed,
                        ready,
                        operations_per_worker,
                    )
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
                suite.queries.len(),
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
                    .map_err(|_| anyhow!("QPS concurrency worker panicked"))?
            })
            .collect()
    })?;

    let mut phase_samples = std::array::from_fn::<_, PHASES, _>(|_| PhaseSamples::default());
    let mut scratch_growths = 0_u64;
    let mut determinism_failures = 0_u64;
    let mut worker_searches = Vec::with_capacity(workers);
    for result in worker_results {
        scratch_growths += result.scratch_growths;
        determinism_failures += result.determinism_failures;
        worker_searches.push(result.searches);
        for (target, source) in phase_samples.iter_mut().zip(result.phases) {
            target.append(source);
        }
    }
    let total_per_phase = workers.saturating_mul(operations_per_worker);
    let first_qps = queries_per_second(total_per_phase, phase_wall_nanos[0]);
    let steady_qps = queries_per_second(total_per_phase, phase_wall_nanos[1]);
    let steady_private_growth = private_usage[2].saturating_sub(private_usage[1]);
    let [_first, steady_samples] = phase_samples;
    let steady = steady_samples.finish();
    let gates = SweepGates {
        engine_p99_at_most_one_millisecond: steady.engine.p99 <= ENGINE_P99_BUDGET_NANOS,
        end_to_end_p99_at_most_two_milliseconds: steady.end_to_end.p99
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

    Ok(WorkerSweepReceipt {
        workers,
        total_queries_per_phase: total_per_phase,
        first_phase_wall_nanos: phase_wall_nanos[0],
        steady_phase_wall_nanos: phase_wall_nanos[1],
        first_queries_per_second: first_qps,
        steady_queries_per_second: steady_qps,
        speedup_vs_first: 0.0,
        parallel_efficiency: 0.0,
        steady_latency_nanos: steady,
        queue_high_water_batches: phase_queue_high_water[1],
        queue_capacity_batches: workers.saturating_mul(queue_batches_per_worker),
        scratch_capacity_growths: scratch_growths,
        determinism_failures,
        worker_searches,
        private_usage_bytes: PrivateUsageReceipt {
            after_worker_warmup: private_usage[0],
            after_first_phase: private_usage[1],
            after_steady_phase: private_usage[2],
            steady_growth: steady_private_growth,
        },
        gates,
    })
}

pub(super) fn dispatch_phase(
    senders: &[Sender<QueryBatch>],
    phase: usize,
    operations_per_worker: usize,
    query_count: usize,
    batch_size: usize,
    high_water: &mut usize,
) -> Result<()> {
    let batches = operations_per_worker.div_ceil(batch_size);
    for batch in 0..batches {
        let start = batch.saturating_mul(batch_size);
        let len = (operations_per_worker - start).min(batch_size);
        for (worker, sender) in senders.iter().enumerate() {
            sender
                .send_blocking(QueryBatch {
                    phase: u8::try_from(phase).map_err(|_| anyhow!("phase overflow"))?,
                    len: u8::try_from(len).map_err(|_| anyhow!("batch length overflow"))?,
                    sequence_start: worker
                        .saturating_mul(operations_per_worker)
                        .saturating_add(start)
                        % query_count,
                    enqueued: Instant::now(),
                })
                .context("dispatch QPS query batch")?;
        }
        *high_water = (*high_water).max(senders.iter().map(Sender::len).sum());
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn worker_loop(
    worker: usize,
    suite: &QualificationSuite,
    index: &phoenix_lexical_qps::QpsIndex,
    expected_rankings: &[Vec<u64>],
    receiver: Receiver<QueryBatch>,
    completed: &[AtomicUsize; PHASES],
    ready: &Barrier,
    operations_per_worker: usize,
) -> Result<WorkerResult> {
    let prepared = suite
        .queries
        .iter()
        .map(PreparedQuery::new)
        .collect::<Vec<_>>();
    let groups = prepared
        .iter()
        .map(PreparedQuery::group_views)
        .collect::<Vec<Vec<QueryGroup<'_>>>>();
    let mut scratch =
        SearchScratch::with_document_capacity(suite.documents.len(), MAXIMUM_QUERY_GROUPS);
    let mut hits = Vec::<SearchHit>::with_capacity(10);
    for (query, groups) in prepared.iter().zip(&groups) {
        query.search(groups, index, &mut scratch, &mut hits)?;
    }
    let mut result = WorkerResult::new(worker, operations_per_worker);
    ready.wait();

    while let Ok(batch) = receiver.recv_blocking() {
        let phase = usize::from(batch.phase);
        let len = usize::from(batch.len);
        for offset in 0..len {
            let query_index = (batch.sequence_start + offset) % prepared.len();
            let queue_nanos = elapsed_nanos(batch.enqueued);
            let started = Instant::now();
            let receipt = prepared[query_index].search(
                &groups[query_index],
                index,
                &mut scratch,
                &mut hits,
            )?;
            let engine_nanos = elapsed_nanos(started);
            let end_to_end_nanos = elapsed_nanos(batch.enqueued);
            result.phases[phase].engine.push(engine_nanos);
            result.phases[phase].queue.push(queue_nanos);
            result.phases[phase].end_to_end.push(end_to_end_nanos);
            result.scratch_growths += u64::from(receipt.allocations_grew);
            result.determinism_failures +=
                u64::from(!ranking_matches(&hits, &expected_rankings[query_index]));
            result.searches += 1;
        }
        completed[phase].fetch_add(len, Ordering::Release);
    }
    Ok(result)
}

pub(super) fn queries_per_second(queries: usize, nanos: u64) -> f64 {
    if nanos == 0 {
        return 0.0;
    }
    queries as f64 * 1_000_000_000.0 / nanos as f64
}

fn assert_send_sync<T: Send + Sync>() {}

#[derive(Clone, Copy)]
pub(super) struct QueryBatch {
    pub(super) phase: u8,
    pub(super) len: u8,
    pub(super) sequence_start: usize,
    pub(super) enqueued: Instant,
}

struct WorkerResult {
    #[allow(dead_code)]
    worker: usize,
    phases: [PhaseSamples; PHASES],
    scratch_growths: u64,
    determinism_failures: u64,
    searches: usize,
}

impl WorkerResult {
    fn new(worker: usize, operations_per_worker: usize) -> Self {
        Self {
            worker,
            phases: std::array::from_fn(|_| PhaseSamples::with_capacity(operations_per_worker)),
            scratch_growths: 0,
            determinism_failures: 0,
            searches: 0,
        }
    }
}

#[derive(Default)]
struct PhaseSamples {
    engine: Vec<u64>,
    queue: Vec<u64>,
    end_to_end: Vec<u64>,
}

impl PhaseSamples {
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

    fn finish(mut self) -> ConcurrentLatencyReceipt {
        ConcurrentLatencyReceipt {
            engine: LatencyPercentiles::from_samples(&mut self.engine),
            queue: LatencyPercentiles::from_samples(&mut self.queue),
            end_to_end: LatencyPercentiles::from_samples(&mut self.end_to_end),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ConcurrencyQualificationReceipt {
    contract: &'static str,
    authority: &'static str,
    suite_path: String,
    suite_bytes: usize,
    suite_sha256: String,
    binary: super::BinaryIdentity,
    logical_processors: usize,
    query_shapes: [&'static str; 4],
    source_kinds: [&'static str; 2],
    operations_per_worker_per_phase: usize,
    phases: usize,
    fixed_batch_size: usize,
    queue_batches_per_worker: usize,
    sweeps: Vec<WorkerSweepReceipt>,
    recommended_workers: usize,
    peak_queries_per_second: f64,
    peak_speedup_vs_first: f64,
    all_absolute_gates_pass: bool,
    peak_speedup_at_least_1_5x: bool,
    qualified_for_bounded_multi_worker_use: bool,
}

#[derive(Debug, Serialize)]
struct WorkerSweepReceipt {
    workers: usize,
    total_queries_per_phase: usize,
    first_phase_wall_nanos: u64,
    steady_phase_wall_nanos: u64,
    first_queries_per_second: f64,
    steady_queries_per_second: f64,
    speedup_vs_first: f64,
    parallel_efficiency: f64,
    steady_latency_nanos: ConcurrentLatencyReceipt,
    queue_high_water_batches: usize,
    queue_capacity_batches: usize,
    scratch_capacity_growths: u64,
    determinism_failures: u64,
    worker_searches: Vec<usize>,
    private_usage_bytes: PrivateUsageReceipt,
    gates: SweepGates,
}

#[derive(Debug, Serialize)]
struct ConcurrentLatencyReceipt {
    engine: LatencyPercentiles,
    queue: LatencyPercentiles,
    end_to_end: LatencyPercentiles,
}

#[derive(Debug, Serialize)]
struct PrivateUsageReceipt {
    after_worker_warmup: usize,
    after_first_phase: usize,
    after_steady_phase: usize,
    steady_growth: usize,
}

#[derive(Debug, Serialize)]
struct SweepGates {
    engine_p99_at_most_one_millisecond: bool,
    end_to_end_p99_at_most_two_milliseconds: bool,
    zero_scratch_capacity_growth: bool,
    deterministic_rankings: bool,
    bounded_queues: bool,
    every_worker_received_exact_share: bool,
    stable_private_memory: bool,
}

impl SweepGates {
    fn all_pass(&self) -> bool {
        self.engine_p99_at_most_one_millisecond
            && self.end_to_end_p99_at_most_two_milliseconds
            && self.zero_scratch_capacity_growth
            && self.deterministic_rankings
            && self.bounded_queues
            && self.every_worker_received_exact_share
            && self.stable_private_memory
    }
}

#[cfg(windows)]
pub(super) fn private_usage_bytes() -> Result<usize> {
    use windows::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
    };
    use windows::Win32::System::Threading::GetCurrentProcess;

    let mut counters = PROCESS_MEMORY_COUNTERS_EX {
        cb: u32::try_from(std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>())
            .map_err(|_| anyhow!("process memory counter size overflow"))?,
        ..Default::default()
    };
    // SAFETY: the current-process pseudo-handle is valid for this call and the
    // initialized EX structure has the required PROCESS_MEMORY_COUNTERS prefix.
    unsafe {
        K32GetProcessMemoryInfo(
            GetCurrentProcess(),
            (&mut counters as *mut PROCESS_MEMORY_COUNTERS_EX).cast::<PROCESS_MEMORY_COUNTERS>(),
            counters.cb,
        )
        .ok()
        .context("query QPS private memory")?;
    }
    Ok(counters.PrivateUsage)
}

#[cfg(not(windows))]
pub(super) fn private_usage_bytes() -> Result<usize> {
    Ok(0)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::run;

    #[test]
    fn shared_index_workers_preserve_rankings_and_capacity() {
        let suite = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../memory-lock/qps-mixed-qualification-v1.json");
        let receipt = run(&suite, &[1, 2], 64, 2, 16).expect("run concurrent qualification");
        assert_eq!(
            receipt.authority,
            "one-qps-index-sharded-bounded-batch-workers"
        );
        assert!(receipt.all_absolute_gates_pass);
        assert_eq!(receipt.sweeps.len(), 2);
        for sweep in receipt.sweeps {
            assert_eq!(sweep.scratch_capacity_growths, 0);
            assert_eq!(sweep.determinism_failures, 0);
            assert!(sweep.gates.every_worker_received_exact_share);
        }
    }
}
