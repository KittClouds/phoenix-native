//! LT9-LA0: delayed replacement-authority laboratory.
//!
//! This is an isolated, deterministic experiment. It does not call QPS,
//! publish a transport artifact, or alter any serving contract. The only
//! question here is whether an eligibility trace can preserve a nominated
//! directed lexical edge until a later independent witness arrives.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const SCHEMA: &str = "phoenix.lexical.lt9-la0/v1";
const ELIGIBILITY_LAMBDA: f32 = 0.85;
const MIN_ELIGIBILITY: f32 = 0.05;
const AUTHORITY_EPSILON: f32 = 1.0e-6;
const DELAYS: [u32; 4] = [0, 2, 4, 8];

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
enum GoldClass {
    Support,
    Contradiction,
    Abstain,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
enum WitnessKind {
    Support,
    Contradiction,
}

#[derive(Clone, Debug, Serialize)]
struct RelationSpec {
    id: String,
    left: String,
    right: String,
    gold: GoldClass,
    delay: u32,
    self_confirmation_probe: bool,
}

#[derive(Clone, Copy, Debug, Serialize)]
enum EventKind {
    Nomination,
    RelevantAbstain,
    Witness(WitnessKind),
    UnrelatedNoise,
}

#[derive(Clone, Debug, Serialize)]
struct Event {
    sequence: u32,
    relation: Option<usize>,
    origin_relation: Option<usize>,
    source: u32,
    delay_bucket: Option<u32>,
    kind: EventKind,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
enum LearnedClass {
    Supported,
    Contradicted,
    Unresolved,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
enum Arm {
    StaticEvidence,
    ImmediateLocal,
    EligibilityDelayed,
}

impl Arm {
    const ALL: [Self; 3] = [
        Self::StaticEvidence,
        Self::ImmediateLocal,
        Self::EligibilityDelayed,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::StaticEvidence => "A_static_lt_evidence",
            Self::ImmediateLocal => "B_immediate_local_update",
            Self::EligibilityDelayed => "C_eligibility_delayed_witness",
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct EdgeState {
    plus: f32,
    minus: f32,
    eligibility: f32,
    nomination_clock: Option<u64>,
    nomination_source: Option<u32>,
    last_eligibility_clock: u64,
}

impl EdgeState {
    fn class(self) -> LearnedClass {
        if self.plus > self.minus + AUTHORITY_EPSILON {
            LearnedClass::Supported
        } else if self.minus > self.plus + AUTHORITY_EPSILON {
            LearnedClass::Contradicted
        } else {
            LearnedClass::Unresolved
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct WitnessReceipt {
    delay: u32,
    accepted: bool,
    correct_owner: bool,
    self_confirmation_rejected: bool,
    authority_gain: f32,
}

#[derive(Default)]
struct RunResult {
    states: Vec<EdgeState>,
    witnesses: Vec<WitnessReceipt>,
    nominations: usize,
    accepted_witnesses: usize,
    ignored_witnesses: usize,
    self_confirmation_rejections: usize,
    relevant_events: usize,
    unrelated_events: usize,
}

#[derive(Debug, Serialize)]
struct DelayReceipt {
    delay: u32,
    witness_events: usize,
    accepted_witnesses: usize,
    correct_accepted_witnesses: usize,
    mean_authority_gain: f64,
}

#[derive(Debug, Serialize)]
struct ArmEvaluation {
    arm: &'static str,
    stream: &'static str,
    correct_support: usize,
    correct_contradiction: usize,
    correct_abstention: usize,
    false_authorizations: usize,
    false_suppressions: usize,
    wrong_polarity: usize,
    total_relations: usize,
    exact_class_accuracy: f64,
    false_authorization_rate: f64,
    false_authorization_given_authorized: f64,
    accepted_witnesses: usize,
    ignored_witnesses: usize,
    self_confirmation_rejections: usize,
    delay_curve: Vec<DelayReceipt>,
    learned: BTreeMap<String, LearnedClass>,
}

#[derive(Debug, Serialize)]
struct ExperimentReceipt {
    schema: &'static str,
    protocol: &'static str,
    hypothesis: &'static str,
    eligibility_lambda: f32,
    minimum_eligibility: f32,
    relevant_opportunity_clock: &'static str,
    independence_rule: &'static str,
    relation_count: usize,
    relation_specs: Vec<RelationSpec>,
    delays: Vec<u32>,
    normal_stream_sha256: String,
    shuffled_stream_sha256: String,
    negative_control: &'static str,
    normal: Vec<ArmEvaluation>,
    shuffled: Vec<ArmEvaluation>,
    outcome: &'static str,
}

fn main() -> Result<()> {
    let output = env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"D:\phoenix-evals\lt9-la0\lt9-la0-receipt.json"));
    let specs = relation_specs();
    let normal = build_stream(&specs, false);
    let shuffled = build_stream(&specs, true);
    let mut normal_results = Vec::with_capacity(Arm::ALL.len());
    let mut shuffled_results = Vec::with_capacity(Arm::ALL.len());
    for arm in Arm::ALL {
        normal_results.push(evaluate(arm, "normal", &specs, &normal));
        shuffled_results.push(evaluate(
            arm,
            "shuffled_confirmation_control",
            &specs,
            &shuffled,
        ));
    }
    let outcome = classify_outcome(&normal_results, &shuffled_results);
    let receipt = ExperimentReceipt {
        schema: SCHEMA,
        protocol: "LT9-LA0 delayed replacement authority; controlled synthetic chronology",
        hypothesis: "eligibility plus a later independent witness can acquire delayed directed-edge authority more safely than immediate local update",
        eligibility_lambda: ELIGIBILITY_LAMBDA,
        minimum_eligibility: MIN_ELIGIBILITY,
        relevant_opportunity_clock: "only nomination, relevant abstain, and witness events for the same directed edge advance n_xy; unrelated noise does not decay eligibility",
        independence_rule: "a witness must occur after nomination and originate from a different source identifier; nomination alone never updates A+ or A-",
        relation_count: specs.len(),
        relation_specs: specs,
        delays: DELAYS.to_vec(),
        normal_stream_sha256: hash_events(&normal)?,
        shuffled_stream_sha256: hash_events(&shuffled)?,
        negative_control: "deterministic seeded ownership shuffle within each delay bucket preserves event counts, delays, and witness polarity totals while reassigning witnesses to other eligible edges",
        normal: normal_results,
        shuffled: shuffled_results,
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

fn relation_specs() -> Vec<RelationSpec> {
    vec![
        spec(
            "repair_to_fix",
            "repair",
            "fix",
            GoldClass::Support,
            0,
            false,
        ),
        spec(
            "fix_to_repair",
            "fix",
            "repair",
            GoldClass::Support,
            0,
            false,
        ),
        spec(
            "engine_to_motor",
            "engine",
            "motor",
            GoldClass::Support,
            2,
            false,
        ),
        spec(
            "motor_to_engine",
            "motor",
            "engine",
            GoldClass::Support,
            2,
            false,
        ),
        spec(
            "car_to_automobile",
            "car",
            "automobile",
            GoldClass::Support,
            4,
            false,
        ),
        spec(
            "car_to_vehicle",
            "car",
            "vehicle",
            GoldClass::Support,
            8,
            false,
        ),
        spec(
            "bank_to_shore",
            "bank",
            "shore",
            GoldClass::Support,
            8,
            false,
        ),
        spec(
            "bank_to_lender",
            "bank",
            "lender",
            GoldClass::Support,
            8,
            false,
        ),
        spec(
            "economic_to_tumor",
            "economic",
            "tumor",
            GoldClass::Contradiction,
            4,
            false,
        ),
        spec(
            "financial_to_medical",
            "financial",
            "medical",
            GoldClass::Contradiction,
            8,
            false,
        ),
        spec(
            "sword_to_banana",
            "sword",
            "banana",
            GoldClass::Abstain,
            4,
            false,
        ),
        spec(
            "theory_to_fact_self_probe",
            "theory",
            "fact",
            GoldClass::Abstain,
            0,
            true,
        ),
    ]
}

fn spec(
    id: &str,
    left: &str,
    right: &str,
    gold: GoldClass,
    delay: u32,
    self_confirmation_probe: bool,
) -> RelationSpec {
    RelationSpec {
        id: id.to_owned(),
        left: left.to_owned(),
        right: right.to_owned(),
        gold,
        delay,
        self_confirmation_probe,
    }
}

fn build_stream(specs: &[RelationSpec], shuffle_ownership: bool) -> Vec<Event> {
    let mut events = Vec::new();
    let mut sequence = 0_u32;
    let mut push =
        |events: &mut Vec<Event>, relation, origin_relation, source, delay_bucket, kind| {
            events.push(Event {
                sequence,
                relation,
                origin_relation,
                source,
                delay_bucket,
                kind,
            });
            sequence += 1;
        };

    for (relation, _) in specs.iter().enumerate() {
        push(
            &mut events,
            Some(relation),
            None,
            10_000 + relation as u32,
            None,
            EventKind::Nomination,
        );
        push(
            &mut events,
            None,
            None,
            90_000 + relation as u32,
            None,
            EventKind::UnrelatedNoise,
        );
    }

    for delay in DELAYS {
        for (relation, current) in specs
            .iter()
            .enumerate()
            .filter(|(_, item)| item.delay == delay)
        {
            for distractor in 0..delay {
                push(
                    &mut events,
                    Some(relation),
                    None,
                    20_000 + relation as u32 * 100 + distractor,
                    Some(delay),
                    EventKind::RelevantAbstain,
                );
                let source = 80_000 + events.len() as u32;
                push(
                    &mut events,
                    None,
                    None,
                    source,
                    None,
                    EventKind::UnrelatedNoise,
                );
            }
            let witness = if current.self_confirmation_probe {
                Some(WitnessKind::Support)
            } else {
                match current.gold {
                    GoldClass::Support => Some(WitnessKind::Support),
                    GoldClass::Contradiction => Some(WitnessKind::Contradiction),
                    GoldClass::Abstain => None,
                }
            };
            if let Some(kind) = witness {
                let source = if current.self_confirmation_probe {
                    10_000 + relation as u32
                } else {
                    30_000 + relation as u32
                };
                push(
                    &mut events,
                    Some(relation),
                    Some(relation),
                    source,
                    Some(delay),
                    EventKind::Witness(kind),
                );
            }
            let source = 70_000 + events.len() as u32;
            push(
                &mut events,
                None,
                None,
                source,
                None,
                EventKind::UnrelatedNoise,
            );
        }
    }
    if shuffle_ownership {
        shuffle_witness_ownership(&mut events);
    }
    events
}

fn shuffle_witness_ownership(events: &mut [Event]) {
    let mut state = 0x9E37_79B9_7F4A_7C15_u64;
    for delay in DELAYS {
        let indices = events
            .iter()
            .enumerate()
            .filter_map(|(index, event)| {
                (event.delay_bucket == Some(delay)
                    && matches!(event.kind, EventKind::Witness(_))
                    && event
                        .relation
                        .zip(event.origin_relation)
                        .is_some_and(|(relation, origin)| relation == origin))
                .then_some(index)
            })
            .collect::<Vec<_>>();
        if indices.len() < 2 {
            continue;
        }
        let mut destinations = indices
            .iter()
            .map(|&index| events[index].relation.expect("witness relation"))
            .collect::<Vec<_>>();
        for cursor in (1..destinations.len()).rev() {
            state ^= state << 7;
            state ^= state >> 9;
            state ^= state << 8;
            let swap = (state as usize) % (cursor + 1);
            destinations.swap(cursor, swap);
        }
        if destinations
            .iter()
            .zip(indices.iter())
            .any(|(destination, index)| Some(*destination) == events[*index].origin_relation)
        {
            destinations.rotate_left(1);
        }
        for (index, destination) in indices.into_iter().zip(destinations) {
            events[index].relation = Some(destination);
        }
    }
}

fn evaluate(
    arm: Arm,
    stream_label: &'static str,
    specs: &[RelationSpec],
    events: &[Event],
) -> ArmEvaluation {
    let result = run_arm(arm, specs.len(), events);
    let mut correct_support = 0;
    let mut correct_contradiction = 0;
    let mut correct_abstention = 0;
    let mut false_authorizations = 0;
    let mut false_suppressions = 0;
    let mut wrong_polarity = 0;
    let mut learned = BTreeMap::new();
    for (index, state) in result.states.iter().copied().enumerate() {
        let learned_class = state.class();
        learned.insert(specs[index].id.clone(), learned_class);
        match (specs[index].gold, learned_class) {
            (GoldClass::Support, LearnedClass::Supported) => correct_support += 1,
            (GoldClass::Contradiction, LearnedClass::Contradicted) => correct_contradiction += 1,
            (GoldClass::Abstain, LearnedClass::Unresolved) => correct_abstention += 1,
            (GoldClass::Support | GoldClass::Contradiction, LearnedClass::Unresolved) => {
                false_suppressions += 1
            }
            (GoldClass::Support, LearnedClass::Contradicted)
            | (GoldClass::Contradiction, LearnedClass::Supported) => {
                wrong_polarity += 1;
                false_authorizations += 1;
            }
            (GoldClass::Abstain, LearnedClass::Supported | LearnedClass::Contradicted) => {
                false_authorizations += 1
            }
        }
    }
    let correct = correct_support + correct_contradiction + correct_abstention;
    let authorized = specs
        .iter()
        .zip(result.states.iter())
        .filter(|(_, state)| !matches!(state.class(), LearnedClass::Unresolved))
        .count();
    let delay_curve = DELAYS
        .into_iter()
        .map(|delay| {
            let items = result
                .witnesses
                .iter()
                .filter(|item| item.delay == delay)
                .collect::<Vec<_>>();
            let gains = items
                .iter()
                .map(|item| f64::from(item.authority_gain))
                .sum::<f64>();
            let accepted = items.iter().filter(|item| item.accepted).count();
            DelayReceipt {
                delay,
                witness_events: items.len(),
                accepted_witnesses: accepted,
                correct_accepted_witnesses: items
                    .iter()
                    .filter(|item| item.accepted && item.correct_owner)
                    .count(),
                mean_authority_gain: if accepted == 0 {
                    0.0
                } else {
                    gains / accepted as f64
                },
            }
        })
        .collect();
    ArmEvaluation {
        arm: arm.label(),
        stream: stream_label,
        correct_support,
        correct_contradiction,
        correct_abstention,
        false_authorizations,
        false_suppressions,
        wrong_polarity,
        total_relations: specs.len(),
        exact_class_accuracy: correct as f64 / specs.len() as f64,
        false_authorization_rate: false_authorizations as f64 / specs.len() as f64,
        false_authorization_given_authorized: if authorized == 0 {
            0.0
        } else {
            false_authorizations as f64 / authorized as f64
        },
        accepted_witnesses: result.accepted_witnesses,
        ignored_witnesses: result.ignored_witnesses,
        self_confirmation_rejections: result.self_confirmation_rejections,
        delay_curve,
        learned,
    }
}

fn run_arm(arm: Arm, relation_count: usize, events: &[Event]) -> RunResult {
    let mut states = vec![EdgeState::default(); relation_count];
    let mut result = RunResult {
        states: states.clone(),
        ..RunResult::default()
    };
    let mut clocks = vec![0_u64; relation_count];
    for event in events {
        let Some(relation) = event.relation else {
            result.unrelated_events += 1;
            continue;
        };
        match event.kind {
            EventKind::Nomination => {
                result.nominations += 1;
                clocks[relation] += 1;
                let state = &mut states[relation];
                state.nomination_clock = Some(clocks[relation]);
                state.nomination_source = Some(event.source);
                state.eligibility = 1.0;
                state.last_eligibility_clock = clocks[relation];
            }
            EventKind::RelevantAbstain => {
                result.relevant_events += 1;
                clocks[relation] += 1;
            }
            EventKind::Witness(kind) => {
                result.relevant_events += 1;
                clocks[relation] += 1;
                let state = &mut states[relation];
                let nomination_clock = state.nomination_clock;
                let gap = nomination_clock
                    .map(|clock| clocks[relation].saturating_sub(clock).saturating_sub(1))
                    .unwrap_or(0);
                let correct_owner = event.origin_relation == Some(relation);
                let self_confirmation = state.nomination_source == Some(event.source);
                let mut receipt = WitnessReceipt {
                    delay: event.delay_bucket.unwrap_or(0),
                    correct_owner,
                    self_confirmation_rejected: false,
                    ..WitnessReceipt::default()
                };
                let accepted = match arm {
                    Arm::StaticEvidence => true,
                    Arm::ImmediateLocal => gap == 0,
                    Arm::EligibilityDelayed => {
                        let decay = ELIGIBILITY_LAMBDA.powi(i32::try_from(gap).unwrap_or(i32::MAX));
                        state.eligibility *= decay;
                        !self_confirmation && state.eligibility >= MIN_ELIGIBILITY
                    }
                };
                if arm == Arm::EligibilityDelayed && self_confirmation {
                    receipt.self_confirmation_rejected = true;
                    result.self_confirmation_rejections += 1;
                }
                if accepted {
                    let gain = if arm == Arm::EligibilityDelayed {
                        state.eligibility
                    } else {
                        1.0
                    };
                    match kind {
                        WitnessKind::Support => state.plus += gain,
                        WitnessKind::Contradiction => state.minus += gain,
                    }
                    receipt.accepted = true;
                    receipt.authority_gain = gain;
                    result.accepted_witnesses += 1;
                    if arm == Arm::EligibilityDelayed {
                        state.eligibility = 0.0;
                    }
                } else {
                    result.ignored_witnesses += 1;
                }
                result.witnesses.push(receipt);
                if arm != Arm::EligibilityDelayed {
                    state.eligibility = 0.0;
                }
            }
            EventKind::UnrelatedNoise => unreachable!("unrelated event without relation"),
        }
    }
    result.states = states;
    result
}

fn classify_outcome(normal: &[ArmEvaluation], shuffled: &[ArmEvaluation]) -> &'static str {
    let normal_c = normal
        .iter()
        .find(|item| item.arm == Arm::EligibilityDelayed.label());
    let shuffled_c = shuffled
        .iter()
        .find(|item| item.arm == Arm::EligibilityDelayed.label());
    let normal_b = normal
        .iter()
        .find(|item| item.arm == Arm::ImmediateLocal.label());
    match (normal_b, normal_c, shuffled_c) {
        (Some(b), Some(c), Some(shuffled))
            if c.exact_class_accuracy > b.exact_class_accuracy
                && shuffled.exact_class_accuracy < c.exact_class_accuracy
                && shuffled.false_authorizations > 0 =>
        {
            "PASS: delayed eligibility outperforms immediate local update and degrades under ownership shuffle"
        }
        _ => "INCONCLUSIVE: protocol ran, but the predeclared mechanism gates were not all met",
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
    fn nomination_alone_cannot_authorize() {
        let specs = relation_specs();
        let events = specs
            .iter()
            .enumerate()
            .map(|(relation, _)| Event {
                sequence: relation as u32,
                relation: Some(relation),
                origin_relation: None,
                source: 50_000 + relation as u32,
                delay_bucket: None,
                kind: EventKind::Nomination,
            })
            .collect::<Vec<_>>();
        let result = run_arm(Arm::EligibilityDelayed, specs.len(), &events);
        assert!(result
            .states
            .iter()
            .all(|state| state.class() == LearnedClass::Unresolved));
        assert_eq!(result.accepted_witnesses, 0);
    }

    #[test]
    fn delayed_independent_witness_beats_immediate_local_update() {
        let specs = relation_specs();
        let events = build_stream(&specs, false);
        let immediate = evaluate(Arm::ImmediateLocal, "normal", &specs, &events);
        let delayed = evaluate(Arm::EligibilityDelayed, "normal", &specs, &events);
        assert!(delayed.exact_class_accuracy > immediate.exact_class_accuracy);
        assert_eq!(delayed.false_authorizations, 0);
        assert_eq!(delayed.self_confirmation_rejections, 1);
    }

    #[test]
    fn ownership_shuffle_breaks_delayed_authority() {
        let specs = relation_specs();
        let events = build_stream(&specs, true);
        let shuffled = evaluate(
            Arm::EligibilityDelayed,
            "shuffled_confirmation_control",
            &specs,
            &events,
        );
        assert!(shuffled.false_authorizations > 0);
        assert!(shuffled.exact_class_accuracy < 1.0);
    }
}
