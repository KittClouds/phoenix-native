//! LT9-LA1: contextual authority compartments.
//!
//! This remains an isolated laboratory. It does not call QPS, publish a
//! transport artifact, or change any serving policy. LA0's independent-witness
//! contract is held fixed while context partitioning and delayed eligibility
//! are compared.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context as AnyhowContext, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};

const SCHEMA: &str = "phoenix.lexical.lt9-la1/v1";
const ELIGIBILITY_LAMBDA: f32 = 0.85;
const MIN_ELIGIBILITY: f32 = 0.05;
const EPSILON: f32 = 1.0e-6;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
enum Context {
    Geography,
    Finance,
    Biology,
    Telecom,
    Animal,
    Computing,
    General,
    Merged,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
enum Phase {
    AcquireA,
    SwitchB,
    SustainB,
    ReturnA,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
enum GoldClass {
    Support,
    Contradiction,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
enum WitnessKind {
    Support,
    Contradiction,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
enum LearnedClass {
    Supported,
    Contradicted,
    Unresolved,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
enum Arm {
    GlobalImmediate,
    PhenotypeImmediate,
    PhenotypeEligibility,
}

impl Arm {
    const fn label(self) -> &'static str {
        match self {
            Self::GlobalImmediate => "A_global_immediate",
            Self::PhenotypeImmediate => "B_phenotype_immediate",
            Self::PhenotypeEligibility => "C_phenotype_eligibility",
        }
    }

    const fn partitioned(self) -> bool {
        !matches!(self, Self::GlobalImmediate)
    }

    const fn eligibility(self) -> bool {
        matches!(self, Self::PhenotypeEligibility)
    }
}

#[derive(Clone, Debug, Serialize)]
struct TruthSpec {
    id: String,
    relation: usize,
    relation_label: String,
    context: Context,
    gold: GoldClass,
    role: &'static str,
    initial_delay: u32,
}

#[derive(Clone, Copy, Debug, Serialize)]
enum EventKind {
    Nomination,
    RelevantAbstain,
    Witness(WitnessKind),
    Checkpoint(Phase),
    Probe(Phase, usize),
    UnrelatedNoise,
}

#[derive(Clone, Debug, Serialize)]
struct Event {
    sequence: u32,
    relation: Option<usize>,
    context: Option<Context>,
    origin_relation: Option<usize>,
    origin_context: Option<Context>,
    source: u32,
    delay_bucket: Option<u32>,
    kind: EventKind,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct StateKey {
    relation: usize,
    context: Context,
}

#[derive(Clone, Copy, Debug, Default)]
struct EdgeState {
    plus: f32,
    minus: f32,
    eligibility: f32,
    nomination_clock: Option<u64>,
    nomination_source: Option<u32>,
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

#[derive(Clone, Copy, Debug, Default)]
struct WitnessReceipt {
    accepted: bool,
    correct_owner: bool,
    authority_gain: f32,
}

#[derive(Default)]
struct RunResult {
    states: BTreeMap<StateKey, EdgeState>,
    probes: Vec<ProbeReceipt>,
    snapshots: BTreeMap<Phase, BTreeMap<StateKey, f32>>,
    witnesses: Vec<WitnessReceipt>,
    accepted_witnesses: usize,
    ignored_witnesses: usize,
    self_confirmation_rejections: usize,
}

#[derive(Debug, Serialize)]
struct ProbeReceipt {
    phase: Phase,
    truth_id: String,
    expected: GoldClass,
    learned: LearnedClass,
    correct: bool,
    wrong_context_leak: bool,
}

#[derive(Debug, Serialize)]
struct PhaseMetric {
    phase: Phase,
    probes: usize,
    correct: usize,
    unresolved: usize,
    wrong_context_leaks: usize,
    accuracy: f64,
}

#[derive(Debug, Serialize)]
struct ArmEvaluation {
    arm: &'static str,
    stream: &'static str,
    phase_metrics: Vec<PhaseMetric>,
    context_retention: f64,
    switch_acquisition: f64,
    return_recovery: f64,
    cross_context_leakage: f64,
    inactive_compartment_collateral_mean: f64,
    inactive_compartment_collateral_max: f64,
    accepted_witnesses: usize,
    ignored_witnesses: usize,
    self_confirmation_rejections: usize,
    correct_owner_witnesses: usize,
    wrong_owner_witnesses: usize,
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
    clock_rule: &'static str,
    independence_rule: &'static str,
    relation_count: usize,
    truth_specs: Vec<TruthSpec>,
    normal_stream_sha256: String,
    phenotype_shuffle_stream_sha256: String,
    ownership_shuffle_stream_sha256: String,
    normal: Vec<ArmEvaluation>,
    phenotype_shuffle: ArmEvaluation,
    phenotype_merge: ArmEvaluation,
    ownership_shuffle: ArmEvaluation,
    outcome: &'static str,
}

fn main() -> Result<()> {
    let output = env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"D:\phoenix-evals\lt9-la1\lt9-la1-receipt.json"));
    let (relations, truths) = truth_specs();
    let normal = build_stream(&truths, false);
    let phenotype_shuffle = shuffle_phenotypes(&normal);
    let ownership_shuffle = shuffle_witness_ownership(&normal);
    let normal_evaluations = [
        Arm::GlobalImmediate,
        Arm::PhenotypeImmediate,
        Arm::PhenotypeEligibility,
    ]
    .into_iter()
    .map(|arm| evaluate(arm, "normal", &truths, &normal))
    .collect::<Vec<_>>();
    let phenotype_shuffle_eval = evaluate(
        Arm::PhenotypeEligibility,
        "phenotype_shuffle",
        &truths,
        &phenotype_shuffle,
    );
    let phenotype_merge_eval = evaluate_with_partition(
        Arm::PhenotypeEligibility,
        "phenotype_merge",
        &truths,
        &normal,
        false,
    );
    let ownership_shuffle_eval = evaluate(
        Arm::PhenotypeEligibility,
        "ownership_shuffle_within_phenotype",
        &truths,
        &ownership_shuffle,
    );
    let outcome = classify_outcome(
        &normal_evaluations,
        &phenotype_shuffle_eval,
        &ownership_shuffle_eval,
    );
    let receipt = ExperimentReceipt {
        schema: SCHEMA,
        protocol: "LT9-LA1 contextual authority compartments; acquire/switch/sustain/return chronology",
        hypothesis: "phenotype-specific authority plus delayed eligibility retains valid context-dependent relations through context switches with low cross-context leakage",
        eligibility_lambda: ELIGIBILITY_LAMBDA,
        minimum_eligibility: MIN_ELIGIBILITY,
        clock_rule: "eligibility decays only on relevant opportunities for the active relation/context key; unrelated events and other phenotypes do not advance that key's clock",
        independence_rule: "every accepted witness must follow nomination and originate from a different source identifier; nomination alone never changes authority",
        relation_count: relations.len(),
        truth_specs: truths.clone(),
        normal_stream_sha256: hash_events(&normal)?,
        phenotype_shuffle_stream_sha256: hash_events(&phenotype_shuffle)?,
        ownership_shuffle_stream_sha256: hash_events(&ownership_shuffle)?,
        normal: normal_evaluations,
        phenotype_shuffle: phenotype_shuffle_eval,
        phenotype_merge: phenotype_merge_eval,
        ownership_shuffle: ownership_shuffle_eval,
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

fn truth_specs() -> (Vec<String>, Vec<TruthSpec>) {
    let mut relations = Vec::<String>::new();
    let mut add = |label: &str| {
        if !relations.iter().any(|value| value == label) {
            relations.push(label.to_owned());
        }
        relations.iter().position(|value| value == label).unwrap()
    };
    let mut truths = Vec::new();
    let mut push = |id: &str, label: &str, context, gold, role, initial_delay| {
        truths.push(TruthSpec {
            id: id.to_owned(),
            relation: add(label),
            relation_label: label.to_owned(),
            context,
            gold,
            role,
            initial_delay,
        });
    };
    push(
        "bank_shore_geo",
        "bank_to_shore",
        Context::Geography,
        GoldClass::Support,
        "OLD_A",
        0,
    );
    push(
        "cell_membrane_bio",
        "cell_to_membrane",
        Context::Biology,
        GoldClass::Support,
        "OLD_A",
        0,
    );
    push(
        "mouse_rodent_animal",
        "mouse_to_rodent",
        Context::Animal,
        GoldClass::Support,
        "OLD_A",
        0,
    );
    push(
        "bank_lender_finance",
        "bank_to_lender",
        Context::Finance,
        GoldClass::Support,
        "NEW_B",
        4,
    );
    push(
        "bank_shore_finance",
        "bank_to_shore",
        Context::Finance,
        GoldClass::Contradiction,
        "NEW_B",
        0,
    );
    push(
        "cell_tower_telecom",
        "cell_to_tower",
        Context::Telecom,
        GoldClass::Support,
        "NEW_B",
        4,
    );
    push(
        "cell_membrane_telecom",
        "cell_to_membrane",
        Context::Telecom,
        GoldClass::Contradiction,
        "NEW_B",
        0,
    );
    push(
        "mouse_device_computing",
        "mouse_to_device",
        Context::Computing,
        GoldClass::Support,
        "NEW_B",
        4,
    );
    push(
        "mouse_rodent_computing",
        "mouse_to_rodent",
        Context::Computing,
        GoldClass::Contradiction,
        "NEW_B",
        0,
    );
    push(
        "repair_fix_general",
        "repair_to_fix",
        Context::General,
        GoldClass::Support,
        "STABLE",
        0,
    );
    push(
        "engine_motor_general",
        "engine_to_motor",
        Context::General,
        GoldClass::Support,
        "STABLE",
        0,
    );
    push(
        "economic_tumor_general",
        "economic_to_tumor",
        Context::General,
        GoldClass::Contradiction,
        "STABLE",
        0,
    );
    (relations, truths)
}

fn build_stream(truths: &[TruthSpec], _shuffle: bool) -> Vec<Event> {
    let mut events = Vec::new();
    let mut sequence = 0_u32;
    for truth in truths.iter().filter(|truth| truth.role != "NEW_B") {
        append_episode(
            &mut events,
            &mut sequence,
            truth,
            Phase::AcquireA,
            truth.initial_delay,
            0,
        );
        if truth.role == "OLD_A" {
            append_episode(&mut events, &mut sequence, truth, Phase::AcquireA, 2, 1);
        }
    }
    checkpoint_and_probe(&mut events, &mut sequence, truths, Phase::AcquireA);
    for truth in truths.iter().filter(|truth| truth.role == "NEW_B") {
        append_episode(
            &mut events,
            &mut sequence,
            truth,
            Phase::SwitchB,
            truth.initial_delay,
            2,
        );
    }
    checkpoint_and_probe(&mut events, &mut sequence, truths, Phase::SwitchB);
    for truth in truths.iter().filter(|truth| truth.role == "NEW_B") {
        append_episode(&mut events, &mut sequence, truth, Phase::SustainB, 8, 3);
    }
    checkpoint_and_probe(&mut events, &mut sequence, truths, Phase::SustainB);
    checkpoint_and_probe(&mut events, &mut sequence, truths, Phase::ReturnA);
    events
}

fn push_event(
    events: &mut Vec<Event>,
    sequence: &mut u32,
    relation: Option<usize>,
    context: Option<Context>,
    origin_relation: Option<usize>,
    origin_context: Option<Context>,
    source: u32,
    delay: Option<u32>,
    kind: EventKind,
) {
    events.push(Event {
        sequence: *sequence,
        relation,
        context,
        origin_relation,
        origin_context,
        source,
        delay_bucket: delay,
        kind,
    });
    *sequence += 1;
}

fn append_episode(
    events: &mut Vec<Event>,
    sequence: &mut u32,
    truth: &TruthSpec,
    _phase: Phase,
    delay: u32,
    ordinal: u32,
) {
    let source = 10_000 + truth.relation as u32 * 100 + ordinal;
    push_event(
        events,
        sequence,
        Some(truth.relation),
        Some(truth.context),
        None,
        None,
        source,
        None,
        EventKind::Nomination,
    );
    for _ in 0..delay {
        push_event(
            events,
            sequence,
            Some(truth.relation),
            Some(truth.context),
            None,
            None,
            20_000 + *sequence,
            Some(delay),
            EventKind::RelevantAbstain,
        );
    }
    let witness = match truth.gold {
        GoldClass::Support => WitnessKind::Support,
        GoldClass::Contradiction => WitnessKind::Contradiction,
    };
    push_event(
        events,
        sequence,
        Some(truth.relation),
        Some(truth.context),
        Some(truth.relation),
        Some(truth.context),
        30_000 + truth.relation as u32 * 100 + ordinal,
        Some(delay),
        EventKind::Witness(witness),
    );
    let source = 90_000 + *sequence;
    push_event(
        events,
        sequence,
        None,
        None,
        None,
        None,
        source,
        None,
        EventKind::UnrelatedNoise,
    );
}

fn checkpoint_and_probe(
    events: &mut Vec<Event>,
    sequence: &mut u32,
    truths: &[TruthSpec],
    phase: Phase,
) {
    push_event(
        events,
        sequence,
        None,
        None,
        None,
        None,
        80_000,
        None,
        EventKind::Checkpoint(phase),
    );
    for (index, truth) in truths.iter().enumerate() {
        push_event(
            events,
            sequence,
            Some(truth.relation),
            Some(truth.context),
            Some(truth.relation),
            Some(truth.context),
            81_000 + index as u32,
            None,
            EventKind::Probe(phase, index),
        );
    }
}

fn shuffle_phenotypes(events: &[Event]) -> Vec<Event> {
    let mut shuffled = events.to_vec();
    let indices = shuffled
        .iter()
        .enumerate()
        .filter_map(|(index, event)| {
            (event.context.is_some()
                && !matches!(
                    event.kind,
                    EventKind::Probe(_, _) | EventKind::Checkpoint(_)
                ))
            .then_some(index)
        })
        .collect::<Vec<_>>();
    let mut values = indices
        .iter()
        .map(|&index| shuffled[index].context.unwrap())
        .collect::<Vec<_>>();
    deterministic_shuffle(&mut values, 0xC0FFEE_1234_5678);
    for (index, context) in indices.into_iter().zip(values) {
        shuffled[index].context = Some(context);
    }
    shuffled
}

fn shuffle_witness_ownership(events: &[Event]) -> Vec<Event> {
    let mut shuffled = events.to_vec();
    let mut state = 0xD1B5_4A32_9A7B_4C15_u64;
    let contexts = [
        Context::Geography,
        Context::Finance,
        Context::Biology,
        Context::Telecom,
        Context::Animal,
        Context::Computing,
        Context::General,
    ];
    for context in contexts {
        for delay in [0_u32, 2, 4, 8] {
            let indices = shuffled
                .iter()
                .enumerate()
                .filter_map(|(index, event)| {
                    (event.context == Some(context)
                        && event.delay_bucket == Some(delay)
                        && matches!(event.kind, EventKind::Witness(_)))
                    .then_some(index)
                })
                .collect::<Vec<_>>();
            if indices.len() < 2 {
                continue;
            }
            let mut destinations = indices
                .iter()
                .map(|&index| shuffled[index].relation.unwrap())
                .collect::<Vec<_>>();
            deterministic_shuffle(&mut destinations, state);
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            if destinations
                .iter()
                .zip(indices.iter())
                .any(|(destination, index)| Some(*destination) == shuffled[*index].origin_relation)
            {
                destinations.rotate_left(1);
            }
            for (index, destination) in indices.into_iter().zip(destinations) {
                shuffled[index].relation = Some(destination);
            }
        }
    }
    shuffled
}

fn deterministic_shuffle<T>(values: &mut [T], mut state: u64) {
    for cursor in (1..values.len()).rev() {
        state ^= state << 7;
        state ^= state >> 9;
        state ^= state << 8;
        values.swap(cursor, (state as usize) % (cursor + 1));
    }
}

fn evaluate(
    arm: Arm,
    stream: &'static str,
    truths: &[TruthSpec],
    events: &[Event],
) -> ArmEvaluation {
    evaluate_with_partition(arm, stream, truths, events, arm.partitioned())
}

fn evaluate_with_partition(
    arm: Arm,
    stream: &'static str,
    truths: &[TruthSpec],
    events: &[Event],
    partitioned: bool,
) -> ArmEvaluation {
    let result = run_arm(arm, truths, events, partitioned);
    let mut phase_metrics = Vec::new();
    for phase in [
        Phase::AcquireA,
        Phase::SwitchB,
        Phase::SustainB,
        Phase::ReturnA,
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
        let leaks = rows.iter().filter(|probe| probe.wrong_context_leak).count();
        phase_metrics.push(PhaseMetric {
            phase,
            probes: rows.len(),
            correct,
            unresolved,
            wrong_context_leaks: leaks,
            accuracy: correct as f64 / rows.len().max(1) as f64,
        });
    }
    let old_a = truths
        .iter()
        .filter(|truth| truth.role == "OLD_A")
        .collect::<Vec<_>>();
    let new_b = truths
        .iter()
        .filter(|truth| truth.role == "NEW_B")
        .collect::<Vec<_>>();
    let phase_correct = |phase: Phase, selected: &[&TruthSpec]| {
        let ids = selected
            .iter()
            .map(|truth| truth.id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        result
            .probes
            .iter()
            .filter(|probe| probe.phase == phase && ids.contains(probe.truth_id.as_str()))
            .filter(|probe| probe.correct)
            .count()
    };
    let retention = phase_correct(Phase::ReturnA, &old_a) as f64 / old_a.len().max(1) as f64;
    let switch = phase_correct(Phase::SwitchB, &new_b) as f64 / new_b.len().max(1) as f64;
    let old_return = phase_correct(Phase::ReturnA, &old_a);
    let leaks = result
        .probes
        .iter()
        .filter(|probe| probe.wrong_context_leak)
        .count();
    let expected_probe_count = result.probes.len().max(1);
    let collateral = collateral_for(&result, &old_a, partitioned);
    let correct_owner_witnesses = result
        .witnesses
        .iter()
        .filter(|witness| witness.correct_owner)
        .count();
    let wrong_owner_witnesses = result
        .witnesses
        .len()
        .saturating_sub(correct_owner_witnesses);
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
        context_retention: retention,
        switch_acquisition: switch,
        return_recovery: old_return as f64 / old_a.len().max(1) as f64,
        cross_context_leakage: leaks as f64 / expected_probe_count as f64,
        inactive_compartment_collateral_mean: collateral.0,
        inactive_compartment_collateral_max: collateral.1,
        accepted_witnesses: result.accepted_witnesses,
        ignored_witnesses: result.ignored_witnesses,
        self_confirmation_rejections: result.self_confirmation_rejections,
        correct_owner_witnesses,
        wrong_owner_witnesses,
        accepted_authority_total,
        probe_receipts: result.probes,
    }
}

fn run_arm(arm: Arm, truths: &[TruthSpec], events: &[Event], partitioned: bool) -> RunResult {
    let mut states = BTreeMap::<StateKey, EdgeState>::new();
    let mut clocks = BTreeMap::<StateKey, u64>::new();
    let mut result = RunResult::default();
    for event in events {
        match event.kind {
            EventKind::UnrelatedNoise => {}
            EventKind::Checkpoint(phase) => {
                result.snapshots.insert(
                    phase,
                    states
                        .iter()
                        .map(|(key, state)| (*key, state.authority()))
                        .collect(),
                );
            }
            EventKind::Probe(phase, truth_index) => {
                let relation = event.relation.expect("probe relation");
                let context = event.context.expect("probe context");
                let key = state_key(relation, context, partitioned);
                let state = states.get(&key).copied().unwrap_or_default();
                let learned = state.class();
                let truth = truths.get(truth_index).expect("probe truth index");
                let expected = truth.gold;
                result.probes.push(ProbeReceipt {
                    phase,
                    truth_id: truth.id.clone(),
                    expected,
                    learned,
                    correct: expected_matches(expected, learned),
                    wrong_context_leak: matches!(
                        (expected, learned),
                        (GoldClass::Support, LearnedClass::Contradicted)
                            | (GoldClass::Contradiction, LearnedClass::Supported)
                    ),
                });
            }
            EventKind::Nomination => {
                let relation = event.relation.expect("nomination relation");
                let context = event.context.expect("nomination context");
                let key = state_key(relation, context, partitioned);
                let clock = clocks.entry(key).or_default();
                *clock += 1;
                let state = states.entry(key).or_default();
                state.nomination_clock = Some(*clock);
                state.nomination_source = Some(event.source);
                if arm.eligibility() {
                    state.eligibility += 1.0;
                }
            }
            EventKind::RelevantAbstain => {
                let relation = event.relation.expect("relevant relation");
                let context = event.context.expect("relevant context");
                let key = state_key(relation, context, partitioned);
                *clocks.entry(key).or_default() += 1;
            }
            EventKind::Witness(kind) => {
                let relation = event.relation.expect("witness relation");
                let context = event.context.expect("witness context");
                let key = state_key(relation, context, partitioned);
                let clock = clocks.entry(key).or_default();
                *clock += 1;
                let state = states.entry(key).or_default();
                let gap = state
                    .nomination_clock
                    .map(|nomination| clock.saturating_sub(nomination).saturating_sub(1))
                    .unwrap_or(0);
                let independent = state.nomination_source != Some(event.source);
                let accepted = if arm.eligibility() {
                    state.eligibility *=
                        ELIGIBILITY_LAMBDA.powi(i32::try_from(gap).unwrap_or(i32::MAX));
                    independent && state.eligibility >= MIN_ELIGIBILITY
                } else {
                    independent && gap == 0
                };
                if !independent {
                    result.self_confirmation_rejections += 1;
                }
                let correct_owner = event.origin_relation == Some(relation)
                    && event.origin_context == Some(context);
                let gain = if accepted {
                    if arm.eligibility() {
                        state.eligibility
                    } else {
                        1.0
                    }
                } else {
                    0.0
                };
                if accepted {
                    match kind {
                        WitnessKind::Support => state.plus += gain,
                        WitnessKind::Contradiction => state.minus += gain,
                    }
                    if arm.eligibility() {
                        state.eligibility = 0.0;
                    }
                    result.accepted_witnesses += 1;
                } else {
                    result.ignored_witnesses += 1;
                }
                result.witnesses.push(WitnessReceipt {
                    accepted,
                    correct_owner,
                    authority_gain: gain,
                });
            }
        }
    }
    result.states = states;
    result
}

fn state_key(relation: usize, context: Context, partitioned: bool) -> StateKey {
    StateKey {
        relation,
        context: if partitioned {
            context
        } else {
            Context::Merged
        },
    }
}

fn expected_matches(expected: GoldClass, learned: LearnedClass) -> bool {
    matches!(
        (expected, learned),
        (GoldClass::Support, LearnedClass::Supported)
            | (GoldClass::Contradiction, LearnedClass::Contradicted)
    )
}

fn collateral_for(result: &RunResult, old_a: &[&TruthSpec], partitioned: bool) -> (f64, f64) {
    let Some(before) = result.snapshots.get(&Phase::AcquireA) else {
        return (0.0, 0.0);
    };
    let Some(after) = result.snapshots.get(&Phase::SustainB) else {
        return (0.0, 0.0);
    };
    let values = old_a
        .iter()
        .map(|truth| {
            let key = state_key(truth.relation, truth.context, partitioned);
            (after.get(&key).copied().unwrap_or(0.0) - before.get(&key).copied().unwrap_or(0.0))
                .abs()
        })
        .collect::<Vec<_>>();
    let mean = values.iter().sum::<f32>() as f64 / values.len().max(1) as f64;
    let max = values.iter().copied().fold(0.0_f32, f32::max) as f64;
    (mean, max)
}

fn classify_outcome(
    normal: &[ArmEvaluation],
    phenotype_shuffle: &ArmEvaluation,
    ownership_shuffle: &ArmEvaluation,
) -> &'static str {
    let global = normal
        .iter()
        .find(|item| item.arm == Arm::GlobalImmediate.label());
    let partitioned = normal
        .iter()
        .find(|item| item.arm == Arm::PhenotypeImmediate.label());
    let eligibility = normal
        .iter()
        .find(|item| item.arm == Arm::PhenotypeEligibility.label());
    match (global, partitioned, eligibility) {
        (Some(global), Some(partitioned), Some(eligibility))
            if partitioned.context_retention > global.context_retention
                && eligibility.switch_acquisition > partitioned.switch_acquisition
                && eligibility.cross_context_leakage <= partitioned.cross_context_leakage
                && phenotype_shuffle.context_retention < eligibility.context_retention
                && ownership_shuffle.cross_context_leakage > eligibility.cross_context_leakage =>
        {
            "PASS: compartments retain context and eligibility adds delayed acquisition; both shuffle controls degrade the mechanism"
        }
        _ => "INCONCLUSIVE: protocol ran, but the predeclared LA1 mechanism gates were not all met",
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
    fn eligibility_preserves_context_and_acquires_delayed_b() {
        let (_, truths) = truth_specs();
        let stream = build_stream(&truths, false);
        let global = evaluate(Arm::GlobalImmediate, "normal", &truths, &stream);
        let partitioned = evaluate(Arm::PhenotypeImmediate, "normal", &truths, &stream);
        let eligibility = evaluate(Arm::PhenotypeEligibility, "normal", &truths, &stream);

        assert_eq!(global.context_retention, 0.0);
        assert_eq!(partitioned.context_retention, 1.0);
        assert_eq!(partitioned.switch_acquisition, 0.5);
        assert_eq!(eligibility.context_retention, 1.0);
        assert_eq!(eligibility.switch_acquisition, 1.0);
        assert_eq!(eligibility.return_recovery, 1.0);
        assert_eq!(eligibility.cross_context_leakage, 0.0);
    }

    #[test]
    fn phenotype_shuffle_degrades_the_partitioned_mechanism() {
        let (_, truths) = truth_specs();
        let stream = build_stream(&truths, false);
        let shuffled = shuffle_phenotypes(&stream);
        let normal = evaluate(Arm::PhenotypeEligibility, "normal", &truths, &stream);
        let shuffled_eval = evaluate(Arm::PhenotypeEligibility, "shuffle", &truths, &shuffled);

        assert!(shuffled_eval.context_retention < normal.context_retention);
        assert!(shuffled_eval.switch_acquisition < normal.switch_acquisition);
    }

    #[test]
    fn ownership_shuffle_increases_wrong_context_leakage() {
        let (_, truths) = truth_specs();
        let stream = build_stream(&truths, false);
        let shuffled = shuffle_witness_ownership(&stream);
        let normal = evaluate(Arm::PhenotypeEligibility, "normal", &truths, &stream);
        let shuffled_eval = evaluate(
            Arm::PhenotypeEligibility,
            "ownership_shuffle",
            &truths,
            &shuffled,
        );

        assert_eq!(normal.cross_context_leakage, 0.0);
        assert!(shuffled_eval.cross_context_leakage > normal.cross_context_leakage);
    }
}
