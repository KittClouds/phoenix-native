//! LT9-LA1-R: true polarity reversal under one fixed phenotype.
//!
//! LA1 proved that contextual compartments isolate concurrently valid edges.
//! This laboratory keeps one context fixed and tests whether authority can
//! move from support to contradiction and back again. It is synthetic and
//! isolated: no QPS artifact or serving policy is changed.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context as AnyhowContext, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};

const SCHEMA: &str = "phoenix.lexical.lt9-la1r/v1";
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
    Nomination(Polarity),
    RelevantAbstain,
    Witness(Polarity),
    Checkpoint(Phase),
    Probe(Phase, usize),
    UnrelatedNoise,
}

#[derive(Clone, Debug, Serialize)]
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

#[derive(Clone, Copy, Debug, Default)]
struct EdgeState {
    plus: f32,
    minus: f32,
    shared_eligibility: f32,
    plus_eligibility: f32,
    minus_eligibility: f32,
    nomination_clock: Option<u64>,
    nomination_source: Option<u32>,
    nomination_polarity: Option<Polarity>,
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
    reversal_latency_mean: f64,
    recovery_latency_mean: f64,
    residual_old_authority_after_reverse: f64,
    overshoot_after_reverse: f64,
    final_authority_mean: f64,
    accepted_witnesses: usize,
    ignored_witnesses: usize,
    self_confirmation_rejections: usize,
    accepted_authority_total: f64,
    probe_receipts: Vec<ProbeReceipt>,
}

#[derive(Debug, Serialize)]
struct ExperimentReceipt {
    schema: &'static str,
    protocol: &'static str,
    hypothesis: &'static str,
    eligibility_lambda: f32,
    minimum_eligibility: f32,
    independence_rule: &'static str,
    edge_specs: Vec<EdgeSpec>,
    normal_stream_sha256: String,
    timing_shuffle_stream_sha256: String,
    normal: Vec<ArmEvaluation>,
    timing_shuffle: Vec<ArmEvaluation>,
    outcome: &'static str,
}

fn main() -> Result<()> {
    let output = env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"D:\phoenix-evals\lt9-la1r\lt9-la1r-receipt.json"));
    let (_, edges) = edge_specs();
    let normal = build_stream(&edges, false);
    let timing_shuffle = build_stream(&edges, true);
    let arms = [Arm::Immediate, Arm::SharedEligibility, Arm::DualEligibility];
    let normal_eval = arms
        .into_iter()
        .map(|arm| evaluate(arm, "normal", &edges, &normal))
        .collect::<Vec<_>>();
    let shuffle_eval = arms
        .into_iter()
        .map(|arm| evaluate(arm, "timing_shuffle", &edges, &timing_shuffle))
        .collect::<Vec<_>>();
    let outcome = classify_outcome(&normal_eval, &shuffle_eval);
    let receipt = ExperimentReceipt {
        schema: SCHEMA,
        protocol: "LT9-LA1-R fixed phenotype; support -> contradiction -> support; reversal timing control",
        hypothesis: "polarity-specific eligibility produces cleaner reversible authority than immediate or shared eligibility",
        eligibility_lambda: ELIGIBILITY_LAMBDA,
        minimum_eligibility: MIN_ELIGIBILITY,
        independence_rule: "accepted witnesses must follow a nomination, have a different source identifier, and satisfy the arm's eligibility rule; silence never updates authority",
        edge_specs: edges.clone(),
        normal_stream_sha256: hash_events(&normal)?,
        timing_shuffle_stream_sha256: hash_events(&timing_shuffle)?,
        normal: normal_eval,
        timing_shuffle: shuffle_eval,
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
            id: format!("reversal_{relation}"),
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
    delay: u32,
    ordinal: u32,
) {
    push_event(
        events,
        sequence,
        Some(relation),
        10_000 + relation as u32 * 100 + ordinal,
        None,
        EventKind::Nomination(polarity),
    );
    for _ in 0..delay {
        push_event(
            events,
            sequence,
            Some(relation),
            20_000 + *sequence,
            Some(delay),
            EventKind::RelevantAbstain,
        );
    }
    push_event(
        events,
        sequence,
        Some(relation),
        30_000 + relation as u32 * 100 + ordinal,
        Some(delay),
        EventKind::Witness(polarity),
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

fn build_stream(edges: &[EdgeSpec], timing_shuffle: bool) -> Vec<Event> {
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
    for ordinal in 0..3 {
        for edge in edges {
            append_episode(
                &mut events,
                &mut sequence,
                edge.relation,
                Polarity::Support,
                2,
                ordinal,
            );
        }
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
    let normal_reverse = [Polarity::Contradiction; 6];
    let shuffled_reverse = [
        Polarity::Contradiction,
        Polarity::Contradiction,
        Polarity::Support,
        Polarity::Contradiction,
        Polarity::Contradiction,
        Polarity::Contradiction,
    ];
    let reverse_order = if timing_shuffle {
        &shuffled_reverse
    } else {
        &normal_reverse
    };
    for (ordinal, polarity) in reverse_order.iter().copied().enumerate() {
        for edge in edges {
            append_episode(
                &mut events,
                &mut sequence,
                edge.relation,
                polarity,
                2,
                10 + ordinal as u32,
            );
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
    for ordinal in 0..sustain_count {
        for edge in edges {
            append_episode(
                &mut events,
                &mut sequence,
                edge.relation,
                Polarity::Contradiction,
                4,
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
    let recovery_count = if timing_shuffle { 4 } else { 5 };
    for ordinal in 0..recovery_count {
        for edge in edges {
            append_episode(
                &mut events,
                &mut sequence,
                edge.relation,
                Polarity::Support,
                2,
                30 + ordinal,
            );
        }
    }
    checkpoint_and_probe(&mut events, &mut sequence, edges, Phase::RecoverA);
    events
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
    let reverse_authority = result
        .snapshots
        .get(&Phase::ReverseB)
        .cloned()
        .unwrap_or_default();
    let residual_old = reverse_authority
        .iter()
        .map(|value| value.max(0.0) as f64)
        .sum::<f64>()
        / reverse_authority.len().max(1) as f64;
    let overshoot = reverse_authority
        .iter()
        .map(|value| value.abs() as f64)
        .sum::<f64>()
        / reverse_authority.len().max(1) as f64;
    let final_authority = result
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
        reversal_latency_mean: mean(&result.reversal_latencies),
        recovery_latency_mean: mean(&result.recovery_latencies),
        residual_old_authority_after_reverse: residual_old,
        overshoot_after_reverse: overshoot,
        final_authority_mean: final_authority,
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
    let mut phase_start = BTreeMap::<Phase, u32>::new();
    let mut first_reversal = BTreeMap::<usize, bool>::new();
    let mut first_recovery = BTreeMap::<usize, bool>::new();
    let mut result = RunResult::default();
    for event in events {
        match event.kind {
            EventKind::UnrelatedNoise => {}
            EventKind::PhaseStart(phase) => {
                phase_start.insert(phase, event.sequence);
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
                let learned = state.class();
                let expected = expected_polarity(phase);
                result.probes.push(ProbeReceipt {
                    phase,
                    edge_id: edge.id.clone(),
                    expected,
                    learned,
                    correct: learned == expected.class(),
                    authority: state.authority(),
                });
            }
            EventKind::Nomination(polarity) => {
                let relation = event.relation.expect("nomination relation");
                let key = StateKey { relation };
                let clock = clocks.entry(key).or_default();
                *clock += 1;
                let state = states.entry(key).or_default();
                state.nomination_clock = Some(*clock);
                state.nomination_source = Some(event.source);
                state.nomination_polarity = Some(polarity);
                match arm {
                    Arm::Immediate => {}
                    Arm::SharedEligibility => state.shared_eligibility += 1.0,
                    Arm::DualEligibility => match polarity {
                        Polarity::Support => state.plus_eligibility += 1.0,
                        Polarity::Contradiction => state.minus_eligibility += 1.0,
                    },
                }
            }
            EventKind::RelevantAbstain => {
                let relation = event.relation.expect("relevant relation");
                *clocks.entry(StateKey { relation }).or_default() += 1;
            }
            EventKind::Witness(polarity) => {
                let relation = event.relation.expect("witness relation");
                let key = StateKey { relation };
                let clock = clocks.entry(key).or_default();
                *clock += 1;
                let state = states.entry(key).or_default();
                let gap = state
                    .nomination_clock
                    .map(|nomination| clock.saturating_sub(nomination).saturating_sub(1))
                    .unwrap_or(u64::MAX);
                let independent = state.nomination_source != Some(event.source);
                if !independent {
                    result.self_confirmation_rejections += 1;
                }
                let gain = match arm {
                    Arm::Immediate => 1.0,
                    Arm::SharedEligibility => {
                        state.shared_eligibility *=
                            ELIGIBILITY_LAMBDA.powi(i32::try_from(gap).unwrap_or(i32::MAX));
                        state.shared_eligibility
                    }
                    Arm::DualEligibility => {
                        let trace = match polarity {
                            Polarity::Support => &mut state.plus_eligibility,
                            Polarity::Contradiction => &mut state.minus_eligibility,
                        };
                        *trace *= ELIGIBILITY_LAMBDA.powi(i32::try_from(gap).unwrap_or(i32::MAX));
                        *trace
                    }
                };
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
                        Arm::SharedEligibility => state.shared_eligibility = 0.0,
                        Arm::DualEligibility => match polarity {
                            Polarity::Support => state.plus_eligibility = 0.0,
                            Polarity::Contradiction => state.minus_eligibility = 0.0,
                        },
                    }
                    result.accepted_witnesses += 1;
                } else {
                    result.ignored_witnesses += 1;
                }
                result.witnesses.push(WitnessReceipt {
                    accepted: eligible,
                    authority_gain: gain,
                });
                if eligible {
                    let class = state.class();
                    let phase = active_phase(event.sequence, &phase_start);
                    if phase == Some(Phase::ReverseB)
                        && class == LearnedClass::Contradicted
                        && !first_reversal.contains_key(&relation)
                    {
                        first_reversal.insert(relation, true);
                        result
                            .reversal_latencies
                            .push(event.sequence - phase_start[&Phase::ReverseB]);
                    }
                    if phase == Some(Phase::RecoverA)
                        && class == LearnedClass::Supported
                        && !first_recovery.contains_key(&relation)
                    {
                        first_recovery.insert(relation, true);
                        result
                            .recovery_latencies
                            .push(event.sequence - phase_start[&Phase::RecoverA]);
                    }
                }
            }
        }
    }
    result
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

fn classify_outcome(normal: &[ArmEvaluation], shuffle: &[ArmEvaluation]) -> &'static str {
    let immediate = normal
        .iter()
        .find(|item| item.arm == Arm::Immediate.label());
    let shared = normal
        .iter()
        .find(|item| item.arm == Arm::SharedEligibility.label());
    let dual = normal
        .iter()
        .find(|item| item.arm == Arm::DualEligibility.label());
    let shared_shuffle = shuffle
        .iter()
        .find(|item| item.arm == Arm::SharedEligibility.label());
    match (immediate, shared, dual, shared_shuffle) {
        (Some(immediate), Some(shared), Some(dual), Some(shared_shuffle))
            if shared.phase_metrics[1].accuracy > immediate.phase_metrics[1].accuracy
                && shared.phase_metrics[3].accuracy > immediate.phase_metrics[3].accuracy
                && dual.phase_metrics[1].accuracy >= shared.phase_metrics[1].accuracy
                && dual.phase_metrics[3].accuracy >= shared.phase_metrics[3].accuracy
                && shared_shuffle.final_authority_mean != shared.final_authority_mean =>
        {
            "PASS: delayed eligibility enables reversal and recovery; timing control changes trajectory; dual trace has no added benefit in this non-overlap stream"
        }
        _ => "INCONCLUSIVE: reversal protocol ran, but predeclared mechanism gates were not all met",
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
    fn delayed_eligibility_reverses_and_recovers() {
        let (_, edges) = edge_specs();
        let stream = build_stream(&edges, false);
        let immediate = evaluate(Arm::Immediate, "normal", &edges, &stream);
        let shared = evaluate(Arm::SharedEligibility, "normal", &edges, &stream);
        let dual = evaluate(Arm::DualEligibility, "normal", &edges, &stream);

        assert!(shared.phase_metrics[1].accuracy > immediate.phase_metrics[1].accuracy);
        assert!(shared.phase_metrics[3].accuracy > immediate.phase_metrics[3].accuracy);
        assert!(dual.phase_metrics[1].accuracy >= shared.phase_metrics[1].accuracy);
        assert!(dual.phase_metrics[3].accuracy >= shared.phase_metrics[3].accuracy);
    }

    #[test]
    fn timing_shuffle_changes_the_history_dependent_trajectory() {
        let (_, edges) = edge_specs();
        let normal = build_stream(&edges, false);
        let shuffled = build_stream(&edges, true);
        let normal_eval = evaluate(Arm::SharedEligibility, "normal", &edges, &normal);
        let shuffled_eval = evaluate(Arm::SharedEligibility, "shuffle", &edges, &shuffled);

        assert_ne!(
            normal_eval.final_authority_mean,
            shuffled_eval.final_authority_mean
        );
    }

    #[test]
    fn deterministic_stream_hashes_are_stable() {
        let (_, edges) = edge_specs();
        let first = build_stream(&edges, false);
        let second = build_stream(&edges, false);
        assert_eq!(hash_events(&first).unwrap(), hash_events(&second).unwrap());
    }
}
