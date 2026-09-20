//! LT9-LA1-RX: overlapping polarity credit.
//!
//! This laboratory holds one phenotype fixed and creates simultaneous pending
//! positive and negative obligations. It compares immediate, shared-trace,
//! and polarity-specific eligibility while preserving witness ownership as an
//! explicit event field. No QPS or serving artifact is touched.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context as AnyhowContext, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};

const SCHEMA: &str = "phoenix.lexical.lt9-la1rx/v1";
const ELIGIBILITY_LAMBDA: f32 = 0.85;
const MIN_ELIGIBILITY: f32 = 0.05;
const EPSILON: f32 = 1.0e-6;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
enum Polarity {
    Support,
    Contradiction,
}

impl Polarity {
    fn class(self) -> LearnedClass {
        match self {
            Self::Support => LearnedClass::Supported,
            Self::Contradiction => LearnedClass::Contradicted,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
enum Phase {
    AcquireA,
    ReverseB,
    SustainB,
    RecoverA,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
enum LearnedClass {
    Supported,
    Contradicted,
    Unresolved,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
enum Arm {
    Immediate,
    SharedEligibility,
    DualEligibility,
}

impl Arm {
    const fn label(self) -> &'static str {
        match self {
            Self::Immediate => "A_immediate",
            Self::SharedEligibility => "B_shared_eligibility",
            Self::DualEligibility => "C_dual_eligibility",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
enum WitnessOrder {
    Fifo,
    Crossed,
}

impl WitnessOrder {
    const fn label(self) -> &'static str {
        match self {
            Self::Fifo => "FIFO",
            Self::Crossed => "CROSSED",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
struct EdgeSpec {
    id: String,
    relation: usize,
    relation_label: String,
    context: &'static str,
}

#[derive(Clone, Copy, Debug, Serialize)]
enum EventKind {
    PhaseStart(Phase),
    Nomination {
        id: u32,
        polarity: Polarity,
    },
    RelevantAbstain,
    Witness {
        target_id: u32,
        intended_id: u32,
        polarity: Polarity,
        overlap: bool,
    },
    Checkpoint(Phase),
    Probe(Phase, usize),
    UnrelatedNoise,
}

#[derive(Clone, Copy, Debug, Serialize)]
struct Event {
    sequence: u32,
    relation: Option<usize>,
    source: u32,
    delay_bucket: Option<u32>,
    kind: EventKind,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct StateKey {
    relation: usize,
}

#[derive(Clone, Copy, Debug)]
struct Pending {
    id: u32,
    polarity: Polarity,
    created_clock: u64,
    source: u32,
}

#[derive(Clone, Copy, Debug, Default)]
struct EdgeState {
    plus: f32,
    minus: f32,
    shared_trace: f32,
    plus_trace: f32,
    minus_trace: f32,
}

impl EdgeState {
    fn class(self) -> LearnedClass {
        if self.plus > self.minus + EPSILON {
            LearnedClass::Supported
        } else if self.minus > self.plus + EPSILON {
            LearnedClass::Contradicted
        } else {
            LearnedClass::Unresolved
        }
    }

    fn authority(self) -> f32 {
        self.plus - self.minus
    }
}

#[derive(Clone, Copy, Debug)]
struct WitnessReceipt {
    accepted: bool,
    authority_gain: f32,
}

#[derive(Default)]
struct RunResult {
    probes: Vec<ProbeReceipt>,
    witnesses: Vec<WitnessReceipt>,
    snapshots: BTreeMap<Phase, Vec<f32>>,
    accepted_witnesses: usize,
    ignored_witnesses: usize,
    self_confirmation_rejections: usize,
    wrong_polarity_updates: usize,
    delayed_updates: usize,
    valid_witnesses: usize,
    lost_witnesses: usize,
    trajectory_error_sum: f64,
    trajectory_steps: usize,
    purity_sum: f64,
    purity_steps: usize,
    reversal_latencies: Vec<u32>,
    recovery_latencies: Vec<u32>,
}

#[derive(Debug, Serialize)]
struct ProbeReceipt {
    phase: Phase,
    edge_id: String,
    expected: Polarity,
    learned: LearnedClass,
    correct: bool,
    authority: f32,
}

#[derive(Debug, Serialize)]
struct PhaseMetric {
    phase: Phase,
    probes: usize,
    correct: usize,
    unresolved: usize,
    accuracy: f64,
    mean_authority: f64,
}

#[derive(Debug, Serialize)]
struct ArmEvaluation {
    arm: &'static str,
    stream: &'static str,
    phase_metrics: Vec<PhaseMetric>,
    wrong_polarity_update_rate: f64,
    lost_witness_rate: f64,
    trajectory_error: f64,
    pending_credit_purity_mean: f64,
    reversal_latency_mean: f64,
    recovery_latency_mean: f64,
    final_authority_mean: f64,
    accepted_witnesses: usize,
    ignored_witnesses: usize,
    self_confirmation_rejections: usize,
    accepted_authority_total: f64,
    probe_receipts: Vec<ProbeReceipt>,
}

#[derive(Debug, Serialize)]
struct ScenarioReceipt {
    overlap_depth: usize,
    witness_order: &'static str,
    normal_stream_sha256: String,
    timing_shuffle_stream_sha256: String,
    normal: Vec<ArmEvaluation>,
    polarity_shuffle: ArmEvaluation,
    ownership_shuffle: ArmEvaluation,
    timing_shuffle: Vec<ArmEvaluation>,
}

#[derive(Debug, Serialize)]
struct ExperimentReceipt {
    schema: &'static str,
    protocol: &'static str,
    hypothesis: &'static str,
    eligibility_lambda: f32,
    minimum_eligibility: f32,
    overlap_depths: Vec<usize>,
    witness_orders: Vec<&'static str>,
    independence_rule: &'static str,
    edge_specs: Vec<EdgeSpec>,
    scenarios: Vec<ScenarioReceipt>,
    outcome: &'static str,
}

fn main() -> Result<()> {
    let output = env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"D:\phoenix-evals\lt9-la1rx\lt9-la1rx-receipt.json"));
    let (_, edges) = edge_specs();
    let depths = [0_usize, 1, 2, 4];
    let orders = [WitnessOrder::Fifo, WitnessOrder::Crossed];
    let mut scenarios = Vec::new();
    for depth in depths {
        for order in orders {
            let normal_stream = build_stream(&edges, depth, order, false);
            let polarity_stream = shuffle_polarity(&normal_stream);
            let ownership_stream = shuffle_ownership(&normal_stream);
            let timing_stream = build_stream(&edges, depth, order, true);
            let arms = [Arm::Immediate, Arm::SharedEligibility, Arm::DualEligibility];
            let normal = arms
                .into_iter()
                .map(|arm| evaluate(arm, "normal", &edges, &normal_stream))
                .collect::<Vec<_>>();
            let timing_shuffle = arms
                .into_iter()
                .map(|arm| evaluate(arm, "timing_shuffle", &edges, &timing_stream))
                .collect::<Vec<_>>();
            scenarios.push(ScenarioReceipt {
                overlap_depth: depth,
                witness_order: order.label(),
                normal_stream_sha256: hash_events(&normal_stream)?,
                timing_shuffle_stream_sha256: hash_events(&timing_stream)?,
                normal,
                polarity_shuffle: evaluate(
                    Arm::DualEligibility,
                    "polarity_shuffle",
                    &edges,
                    &polarity_stream,
                ),
                ownership_shuffle: evaluate(
                    Arm::DualEligibility,
                    "ownership_shuffle",
                    &edges,
                    &ownership_stream,
                ),
                timing_shuffle,
            });
        }
    }
    let outcome = classify_outcome(&scenarios);
    let receipt = ExperimentReceipt {
        schema: SCHEMA,
        protocol: "LT9-LA1-RX fixed phenotype; overlap depths 0/1/2/4; FIFO and CROSSED witness order; polarity, ownership, and count-preserving timing controls",
        hypothesis: "separate positive and negative eligibility traces improve credit assignment when delayed opposite-polarity witnesses overlap",
        eligibility_lambda: ELIGIBILITY_LAMBDA,
        minimum_eligibility: MIN_ELIGIBILITY,
        overlap_depths: depths.to_vec(),
        witness_orders: orders.iter().map(|order| order.label()).collect(),
        independence_rule: "a witness must target a nominated obligation, originate from a different source identifier, and satisfy the arm's eligibility rule; silence never updates authority",
        edge_specs: edges.clone(),
        scenarios,
        outcome,
    };
    let json = serde_json::to_string_pretty(&receipt)?;
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(&output, json.as_bytes()).with_context(|| format!("write {}", output.display()))?;
    println!("{}", json);
    Ok(())
}

fn edge_specs() -> (Vec<String>, Vec<EdgeSpec>) {
    let labels = [
        "repair_to_fix",
        "engine_to_motor",
        "car_to_automobile",
        "bank_to_shore",
    ];
    let relations = labels
        .iter()
        .map(|label| (*label).to_owned())
        .collect::<Vec<_>>();
    let edges = labels
        .iter()
        .enumerate()
        .map(|(relation, label)| EdgeSpec {
            id: format!("rx_edge_{relation}"),
            relation,
            relation_label: (*label).to_owned(),
            context: "fixed_context",
        })
        .collect();
    (relations, edges)
}

fn push_event(
    events: &mut Vec<Event>,
    sequence: &mut u32,
    relation: Option<usize>,
    source: u32,
    delay: Option<u32>,
    kind: EventKind,
) {
    events.push(Event {
        sequence: *sequence,
        relation,
        source,
        delay_bucket: delay,
        kind,
    });
    *sequence += 1;
}

fn append_episode(
    events: &mut Vec<Event>,
    sequence: &mut u32,
    relation: usize,
    polarity: Polarity,
    ordinal: u32,
) {
    let id = 10_000 + relation as u32 * 1_000 + ordinal;
    push_event(
        events,
        sequence,
        Some(relation),
        id,
        Some(2),
        EventKind::Nomination { id, polarity },
    );
    for _ in 0..2 {
        push_event(
            events,
            sequence,
            Some(relation),
            20_000 + *sequence,
            Some(2),
            EventKind::RelevantAbstain,
        );
    }
    push_event(
        events,
        sequence,
        Some(relation),
        30_000 + id,
        Some(2),
        EventKind::Witness {
            target_id: id,
            intended_id: id,
            polarity,
            overlap: false,
        },
    );
    push_event(
        events,
        sequence,
        None,
        90_000 + *sequence,
        None,
        EventKind::UnrelatedNoise,
    );
}

fn append_overlap_block(
    events: &mut Vec<Event>,
    sequence: &mut u32,
    relation: usize,
    depth: usize,
    order: WitnessOrder,
) {
    let mut pairs = Vec::with_capacity(depth);
    for pair in 0..depth {
        let plus_id = 50_000 + relation as u32 * 10_000 + pair as u32 * 2;
        let minus_id = plus_id + 1;
        pairs.push((plus_id, minus_id));
        push_event(
            events,
            sequence,
            Some(relation),
            plus_id,
            Some(99),
            EventKind::Nomination {
                id: plus_id,
                polarity: Polarity::Support,
            },
        );
        push_event(
            events,
            sequence,
            Some(relation),
            minus_id,
            Some(99),
            EventKind::Nomination {
                id: minus_id,
                polarity: Polarity::Contradiction,
            },
        );
    }
    for _ in 0..depth.saturating_mul(2) {
        push_event(
            events,
            sequence,
            Some(relation),
            40_000 + *sequence,
            Some(99),
            EventKind::RelevantAbstain,
        );
    }
    let iter: Box<dyn Iterator<Item = &(u32, u32)>> = match order {
        WitnessOrder::Fifo => Box::new(pairs.iter()),
        WitnessOrder::Crossed => Box::new(pairs.iter().rev()),
    };
    for &(plus_id, minus_id) in iter {
        let (first, second) = match order {
            WitnessOrder::Fifo => (Polarity::Support, Polarity::Contradiction),
            WitnessOrder::Crossed => (Polarity::Contradiction, Polarity::Support),
        };
        let first_id = if first == Polarity::Support {
            plus_id
        } else {
            minus_id
        };
        let second_id = if second == Polarity::Support {
            plus_id
        } else {
            minus_id
        };
        push_event(
            events,
            sequence,
            Some(relation),
            60_000 + first_id,
            Some(99),
            EventKind::Witness {
                target_id: first_id,
                intended_id: first_id,
                polarity: first,
                overlap: true,
            },
        );
        push_event(
            events,
            sequence,
            Some(relation),
            60_000 + second_id,
            Some(99),
            EventKind::Witness {
                target_id: second_id,
                intended_id: second_id,
                polarity: second,
                overlap: true,
            },
        );
    }
}

fn checkpoint_and_probe(
    events: &mut Vec<Event>,
    sequence: &mut u32,
    edges: &[EdgeSpec],
    phase: Phase,
) {
    push_event(
        events,
        sequence,
        None,
        80_000,
        None,
        EventKind::Checkpoint(phase),
    );
    for (index, edge) in edges.iter().enumerate() {
        push_event(
            events,
            sequence,
            Some(edge.relation),
            81_000 + index as u32,
            None,
            EventKind::Probe(phase, index),
        );
    }
}

fn build_stream(
    edges: &[EdgeSpec],
    depth: usize,
    order: WitnessOrder,
    timing_shuffle: bool,
) -> Vec<Event> {
    let mut events = Vec::new();
    let mut sequence = 0_u32;
    push_event(
        &mut events,
        &mut sequence,
        None,
        79_000,
        None,
        EventKind::PhaseStart(Phase::AcquireA),
    );
    for edge in edges {
        append_episode(
            &mut events,
            &mut sequence,
            edge.relation,
            Polarity::Support,
            1,
        );
        append_episode(
            &mut events,
            &mut sequence,
            edge.relation,
            Polarity::Support,
            2,
        );
    }
    checkpoint_and_probe(&mut events, &mut sequence, edges, Phase::AcquireA);
    push_event(
        &mut events,
        &mut sequence,
        None,
        79_001,
        None,
        EventKind::PhaseStart(Phase::ReverseB),
    );
    for edge in edges {
        append_overlap_block(&mut events, &mut sequence, edge.relation, depth, order);
        if timing_shuffle {
            append_episode(
                &mut events,
                &mut sequence,
                edge.relation,
                Polarity::Support,
                100,
            );
            for ordinal in 0..2 {
                append_episode(
                    &mut events,
                    &mut sequence,
                    edge.relation,
                    Polarity::Contradiction,
                    10 + ordinal,
                );
            }
        } else {
            for ordinal in 0..3 {
                append_episode(
                    &mut events,
                    &mut sequence,
                    edge.relation,
                    Polarity::Contradiction,
                    10 + ordinal,
                );
            }
        }
    }
    checkpoint_and_probe(&mut events, &mut sequence, edges, Phase::ReverseB);
    push_event(
        &mut events,
        &mut sequence,
        None,
        79_002,
        None,
        EventKind::PhaseStart(Phase::SustainB),
    );
    let sustain_count = if timing_shuffle { 3 } else { 2 };
    for edge in edges {
        for ordinal in 0..sustain_count {
            append_episode(
                &mut events,
                &mut sequence,
                edge.relation,
                Polarity::Contradiction,
                20 + ordinal,
            );
        }
    }
    checkpoint_and_probe(&mut events, &mut sequence, edges, Phase::SustainB);
    push_event(
        &mut events,
        &mut sequence,
        None,
        79_003,
        None,
        EventKind::PhaseStart(Phase::RecoverA),
    );
    let recovery_count = if timing_shuffle { 3 } else { 4 };
    for edge in edges {
        for ordinal in 0..recovery_count {
            append_episode(
                &mut events,
                &mut sequence,
                edge.relation,
                Polarity::Support,
                30 + ordinal,
            );
        }
    }
    checkpoint_and_probe(&mut events, &mut sequence, edges, Phase::RecoverA);
    events
}

fn shuffle_polarity(events: &[Event]) -> Vec<Event> {
    events
        .iter()
        .copied()
        .map(|mut event| {
            if let EventKind::Witness {
                target_id,
                intended_id,
                polarity,
                overlap: true,
            } = event.kind
            {
                event.kind = EventKind::Witness {
                    target_id,
                    intended_id,
                    polarity: flip(polarity),
                    overlap: true,
                };
            }
            event
        })
        .collect()
}

fn shuffle_ownership(events: &[Event]) -> Vec<Event> {
    let mut shuffled = events.to_vec();
    let indices = shuffled
        .iter()
        .enumerate()
        .filter_map(|(index, event)| {
            matches!(event.kind, EventKind::Witness { overlap: true, .. }).then_some(index)
        })
        .collect::<Vec<_>>();
    let targets = indices
        .iter()
        .filter_map(|&index| match shuffled[index].kind {
            EventKind::Witness { target_id, .. } => Some(target_id),
            _ => None,
        })
        .collect::<Vec<_>>();
    for (offset, &index) in indices.iter().enumerate() {
        let target_id = targets[(offset + 1) % targets.len()];
        if let EventKind::Witness {
            intended_id,
            polarity,
            overlap,
            ..
        } = shuffled[index].kind
        {
            shuffled[index].kind = EventKind::Witness {
                target_id,
                intended_id,
                polarity,
                overlap,
            };
        }
    }
    shuffled
}

fn flip(polarity: Polarity) -> Polarity {
    match polarity {
        Polarity::Support => Polarity::Contradiction,
        Polarity::Contradiction => Polarity::Support,
    }
}

fn evaluate(arm: Arm, stream: &'static str, edges: &[EdgeSpec], events: &[Event]) -> ArmEvaluation {
    let result = run_arm(arm, edges, events);
    let mut phase_metrics = Vec::new();
    for phase in [
        Phase::AcquireA,
        Phase::ReverseB,
        Phase::SustainB,
        Phase::RecoverA,
    ] {
        let rows = result
            .probes
            .iter()
            .filter(|probe| probe.phase == phase)
            .collect::<Vec<_>>();
        let correct = rows.iter().filter(|probe| probe.correct).count();
        let unresolved = rows
            .iter()
            .filter(|probe| probe.learned == LearnedClass::Unresolved)
            .count();
        let mean_authority =
            rows.iter().map(|probe| probe.authority as f64).sum::<f64>() / rows.len().max(1) as f64;
        phase_metrics.push(PhaseMetric {
            phase,
            probes: rows.len(),
            correct,
            unresolved,
            accuracy: correct as f64 / rows.len().max(1) as f64,
            mean_authority,
        });
    }
    let final_authority_mean = result
        .snapshots
        .get(&Phase::RecoverA)
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|value| *value as f64)
        .sum::<f64>()
        / edges.len().max(1) as f64;
    let accepted_authority_total = result
        .witnesses
        .iter()
        .filter(|witness| witness.accepted)
        .map(|witness| witness.authority_gain as f64)
        .sum();
    ArmEvaluation {
        arm: arm.label(),
        stream,
        phase_metrics,
        wrong_polarity_update_rate: result.wrong_polarity_updates as f64
            / result.delayed_updates.max(1) as f64,
        lost_witness_rate: result.lost_witnesses as f64 / result.valid_witnesses.max(1) as f64,
        trajectory_error: result.trajectory_error_sum / result.trajectory_steps.max(1) as f64,
        pending_credit_purity_mean: result.purity_sum / result.purity_steps.max(1) as f64,
        reversal_latency_mean: mean(&result.reversal_latencies),
        recovery_latency_mean: mean(&result.recovery_latencies),
        final_authority_mean,
        accepted_witnesses: result.accepted_witnesses,
        ignored_witnesses: result.ignored_witnesses,
        self_confirmation_rejections: result.self_confirmation_rejections,
        accepted_authority_total,
        probe_receipts: result.probes,
    }
}

fn mean(values: &[u32]) -> f64 {
    values.iter().map(|value| *value as f64).sum::<f64>() / values.len().max(1) as f64
}

fn run_arm(arm: Arm, edges: &[EdgeSpec], events: &[Event]) -> RunResult {
    let mut states = BTreeMap::<StateKey, EdgeState>::new();
    let mut clocks = BTreeMap::<StateKey, u64>::new();
    let mut pending = BTreeMap::<u32, Pending>::new();
    let mut oracle = BTreeMap::<StateKey, f32>::new();
    let mut starts = BTreeMap::<Phase, u32>::new();
    let mut reversed = BTreeMap::<usize, bool>::new();
    let mut recovered = BTreeMap::<usize, bool>::new();
    let mut result = RunResult::default();
    for event in events {
        match event.kind {
            EventKind::UnrelatedNoise => {}
            EventKind::PhaseStart(phase) => {
                starts.insert(phase, event.sequence);
            }
            EventKind::Checkpoint(phase) => {
                result.snapshots.insert(
                    phase,
                    edges
                        .iter()
                        .map(|edge| {
                            states
                                .get(&StateKey {
                                    relation: edge.relation,
                                })
                                .copied()
                                .unwrap_or_default()
                                .authority()
                        })
                        .collect(),
                );
            }
            EventKind::Probe(phase, index) => {
                let edge = edges.get(index).expect("probe edge");
                let state = states
                    .get(&StateKey {
                        relation: edge.relation,
                    })
                    .copied()
                    .unwrap_or_default();
                let expected = expected_polarity(phase);
                result.probes.push(ProbeReceipt {
                    phase,
                    edge_id: edge.id.clone(),
                    expected,
                    learned: state.class(),
                    correct: state.class() == expected.class(),
                    authority: state.authority(),
                });
            }
            EventKind::Nomination { id, polarity } => {
                let relation = event.relation.expect("nomination relation");
                let key = StateKey { relation };
                let clock = clocks.entry(key).or_default();
                *clock += 1;
                pending.insert(
                    id,
                    Pending {
                        id,
                        polarity,
                        created_clock: *clock,
                        source: event.source,
                    },
                );
                let state = states.entry(key).or_default();
                match arm {
                    Arm::Immediate => {}
                    Arm::SharedEligibility => state.shared_trace += 1.0,
                    Arm::DualEligibility => match polarity {
                        Polarity::Support => state.plus_trace += 1.0,
                        Polarity::Contradiction => state.minus_trace += 1.0,
                    },
                }
            }
            EventKind::RelevantAbstain => {
                let relation = event.relation.expect("relevant relation");
                *clocks.entry(StateKey { relation }).or_default() += 1;
            }
            EventKind::Witness {
                target_id,
                intended_id,
                polarity,
                overlap,
            } => {
                let relation = event.relation.expect("witness relation");
                let key = StateKey { relation };
                let clock = clocks.entry(key).or_default();
                *clock += 1;
                let maybe_pending = pending.remove(&target_id);
                let Some(target) = maybe_pending else {
                    update_diagnostics(&mut result, &states, &oracle, &pending, arm, key);
                    continue;
                };
                let gap = clock.saturating_sub(target.created_clock).saturating_sub(1);
                let correct_owner = target.id == intended_id;
                let correct_polarity = target.polarity == polarity;
                if overlap {
                    result.delayed_updates += 1;
                }
                if correct_owner && correct_polarity {
                    result.valid_witnesses += 1;
                }
                if target.source == event.source {
                    result.self_confirmation_rejections += 1;
                }
                let competing_polarity = pending
                    .values()
                    .any(|item| item.polarity != target.polarity);
                let state = states.entry(key).or_default();
                let gain = match arm {
                    Arm::Immediate => 1.0,
                    Arm::SharedEligibility => {
                        state.shared_trace *=
                            ELIGIBILITY_LAMBDA.powi(i32::try_from(gap).unwrap_or(i32::MAX));
                        state.shared_trace
                    }
                    Arm::DualEligibility => {
                        let trace = match target.polarity {
                            Polarity::Support => &mut state.plus_trace,
                            Polarity::Contradiction => &mut state.minus_trace,
                        };
                        *trace *= ELIGIBILITY_LAMBDA.powi(i32::try_from(gap).unwrap_or(i32::MAX));
                        *trace
                    }
                };
                let independent = target.source != event.source;
                let eligible = independent
                    && match arm {
                        Arm::Immediate => gap == 0,
                        Arm::SharedEligibility | Arm::DualEligibility => gain >= MIN_ELIGIBILITY,
                    };
                if eligible {
                    match polarity {
                        Polarity::Support => state.plus += gain,
                        Polarity::Contradiction => state.minus += gain,
                    }
                    match arm {
                        Arm::Immediate => {}
                        Arm::SharedEligibility => state.shared_trace = 0.0,
                        Arm::DualEligibility => match target.polarity {
                            Polarity::Support => state.plus_trace = 0.0,
                            Polarity::Contradiction => state.minus_trace = 0.0,
                        },
                    }
                    result.accepted_witnesses += 1;
                    let shared_alias = arm == Arm::SharedEligibility && competing_polarity;
                    if !correct_owner || !correct_polarity || shared_alias {
                        result.wrong_polarity_updates += 1;
                    }
                } else {
                    result.ignored_witnesses += 1;
                    if correct_owner && correct_polarity {
                        result.lost_witnesses += 1;
                    }
                }
                if correct_owner && correct_polarity {
                    let ideal_gain =
                        ELIGIBILITY_LAMBDA.powi(i32::try_from(gap).unwrap_or(i32::MAX));
                    match target.polarity {
                        Polarity::Support => *oracle.entry(key).or_default() += ideal_gain,
                        Polarity::Contradiction => *oracle.entry(key).or_default() -= ideal_gain,
                    }
                }
                result.witnesses.push(WitnessReceipt {
                    accepted: eligible,
                    authority_gain: gain,
                });
                let phase = active_phase(event.sequence, &starts);
                let class = state.class();
                if phase == Some(Phase::ReverseB)
                    && class == LearnedClass::Contradicted
                    && !reversed.contains_key(&relation)
                {
                    reversed.insert(relation, true);
                    result
                        .reversal_latencies
                        .push(event.sequence - starts[&Phase::ReverseB]);
                }
                if phase == Some(Phase::RecoverA)
                    && class == LearnedClass::Supported
                    && !recovered.contains_key(&relation)
                {
                    recovered.insert(relation, true);
                    result
                        .recovery_latencies
                        .push(event.sequence - starts[&Phase::RecoverA]);
                }
            }
        }
        update_diagnostics(
            &mut result,
            &states,
            &oracle,
            &pending,
            arm,
            event
                .relation
                .map(|relation| StateKey { relation })
                .unwrap_or(StateKey { relation: 0 }),
        );
    }
    result
}

fn update_diagnostics(
    result: &mut RunResult,
    states: &BTreeMap<StateKey, EdgeState>,
    oracle: &BTreeMap<StateKey, f32>,
    pending: &BTreeMap<u32, Pending>,
    arm: Arm,
    key: StateKey,
) {
    let state = states.get(&key).copied().unwrap_or_default();
    let ideal = oracle.get(&key).copied().unwrap_or(0.0);
    result.trajectory_error_sum += (state.authority() - ideal).abs() as f64;
    result.trajectory_steps += 1;
    let plus_pending = pending
        .values()
        .filter(|item| item.polarity == Polarity::Support)
        .count();
    let minus_pending = pending
        .values()
        .filter(|item| item.polarity == Polarity::Contradiction)
        .count();
    let total_trace = match arm {
        Arm::Immediate => 0.0,
        Arm::SharedEligibility => state.shared_trace,
        Arm::DualEligibility => state.plus_trace + state.minus_trace,
    };
    let purity = if total_trace <= EPSILON {
        1.0
    } else {
        match arm {
            Arm::Immediate => 1.0,
            Arm::SharedEligibility => {
                if plus_pending > 0 && minus_pending > 0 {
                    0.5
                } else {
                    1.0
                }
            }
            Arm::DualEligibility => {
                let correct = if plus_pending > 0 {
                    state.plus_trace
                } else {
                    0.0
                } + if minus_pending > 0 {
                    state.minus_trace
                } else {
                    0.0
                };
                (correct / total_trace).clamp(0.0, 1.0)
            }
        }
    };
    result.purity_sum += purity as f64;
    result.purity_steps += 1;
}

fn active_phase(sequence: u32, starts: &BTreeMap<Phase, u32>) -> Option<Phase> {
    starts
        .iter()
        .filter(|(_, start)| **start <= sequence)
        .max_by_key(|(_, start)| **start)
        .map(|(phase, _)| *phase)
}

fn expected_polarity(phase: Phase) -> Polarity {
    match phase {
        Phase::AcquireA | Phase::RecoverA => Polarity::Support,
        Phase::ReverseB | Phase::SustainB => Polarity::Contradiction,
    }
}

fn classify_outcome(scenarios: &[ScenarioReceipt]) -> &'static str {
    let mut base_equal = true;
    let mut dual_better = 0_usize;
    for scenario in scenarios {
        let shared = scenario
            .normal
            .iter()
            .find(|item| item.arm == Arm::SharedEligibility.label());
        let dual = scenario
            .normal
            .iter()
            .find(|item| item.arm == Arm::DualEligibility.label());
        if let (Some(shared), Some(dual)) = (shared, dual) {
            if scenario.overlap_depth == 0 {
                base_equal &= (shared.trajectory_error - dual.trajectory_error).abs() < 1.0e-9
                    && (shared.lost_witness_rate - dual.lost_witness_rate).abs() < 1.0e-9;
            } else if dual.wrong_polarity_update_rate < shared.wrong_polarity_update_rate
                && dual.lost_witness_rate < shared.lost_witness_rate
            {
                dual_better += 1;
            }
        }
    }
    if base_equal && dual_better >= 2 {
        "PASS: shared and dual traces agree at zero overlap; dual traces reduce overlap credit loss and wrong-polarity updates"
    } else if base_equal {
        "INCONCLUSIVE: zero-overlap replication passed, but dual traces did not beat shared traces at two nonzero overlap levels"
    } else {
        "FAIL CLOSED: shared and dual traces diverged before polarity overlap was introduced"
    }
}

fn hash_events(events: &[Event]) -> Result<String> {
    let bytes = serde_json::to_vec(events)?;
    let digest = Sha256::digest(bytes);
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_overlap_shared_and_dual_match() {
        let (_, edges) = edge_specs();
        let stream = build_stream(&edges, 0, WitnessOrder::Fifo, false);
        let shared = evaluate(Arm::SharedEligibility, "normal", &edges, &stream);
        let dual = evaluate(Arm::DualEligibility, "normal", &edges, &stream);
        assert_eq!(shared.lost_witness_rate, dual.lost_witness_rate);
        assert_eq!(shared.trajectory_error, dual.trajectory_error);
    }

    #[test]
    fn overlap_makes_shared_trace_lose_owned_witnesses() {
        let (_, edges) = edge_specs();
        let stream = build_stream(&edges, 2, WitnessOrder::Crossed, false);
        let shared = evaluate(Arm::SharedEligibility, "normal", &edges, &stream);
        let dual = evaluate(Arm::DualEligibility, "normal", &edges, &stream);
        assert!(shared.wrong_polarity_update_rate > 0.0);
        assert!(dual.lost_witness_rate < shared.lost_witness_rate);
        assert!(dual.trajectory_error < shared.trajectory_error);
    }

    #[test]
    fn polarity_shuffle_is_detected_by_dual_trace() {
        let (_, edges) = edge_specs();
        let stream = build_stream(&edges, 2, WitnessOrder::Fifo, false);
        let shuffled = shuffle_polarity(&stream);
        let clean = evaluate(Arm::DualEligibility, "normal", &edges, &stream);
        let control = evaluate(Arm::DualEligibility, "polarity_shuffle", &edges, &shuffled);
        assert!(control.wrong_polarity_update_rate > clean.wrong_polarity_update_rate);
    }
}
