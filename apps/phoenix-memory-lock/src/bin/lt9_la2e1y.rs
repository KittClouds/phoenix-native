//! LT9-LA2-E1Y: bounded credit-routing stress assay.
//!
//! Diagnostic-only. The semantic expiry policy is frozen in
//! `docs/LT9_LA2_E1Y_PROTOCOL.md`: deadline after 32 logical ticks,
//! same-owner opposite-polarity supersession, and bounded-capacity eviction.
//! This binary does not change LA2-B, the memory contract, or serving.

use std::env;
use std::fs;
use std::path::Path;

use anyhow::Result;
use serde::Serialize;
use sha2::{Digest, Sha256};

const SCHEMA: &str = "phoenix.lexical.lt9-la2e1y/v1";
const LAMBDA: f32 = 0.85;
const MIN_TRACE: f32 = 0.01;
const DEADLINE: u32 = 32;
const PENDING_CAPACITY: usize = 7;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
enum Polarity {
    Support,
    Contradiction,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
enum Arm {
    TraceOnly,
    BinaryPending,
    PendingConfidence,
    PendingConfidenceExpiry,
}

impl Arm {
    const ALL: [Self; 4] = [
        Self::TraceOnly,
        Self::BinaryPending,
        Self::PendingConfidence,
        Self::PendingConfidenceExpiry,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::TraceOnly => "trace_only",
            Self::BinaryPending => "binary_pending",
            Self::PendingConfidence => "pending_plus_confidence",
            Self::PendingConfidenceExpiry => "pending_plus_confidence_plus_expiry",
        }
    }

    const fn bounded(self) -> bool {
        !matches!(self, Self::TraceOnly)
    }

    const fn expires(self) -> bool {
        matches!(self, Self::PendingConfidenceExpiry)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
enum ExpiryCause {
    Deadline,
    SupersededByOpposite,
    CapacityEviction,
}

#[derive(Clone, Copy, Debug)]
enum EventKind {
    Nominate {
        id: &'static str,
        slot: u8,
        owner: u8,
        polarity: Polarity,
        confidence: f32,
    },
    Witness {
        slot: u8,
        polarity: Polarity,
        intended: &'static str,
    },
}

#[derive(Clone, Copy, Debug)]
struct Event {
    tick: u32,
    kind: EventKind,
}

const STREAM: &[Event] = &[
    Event {
        tick: 0,
        kind: EventKind::Nominate {
            id: "old_support",
            slot: 0,
            owner: 0,
            polarity: Polarity::Support,
            confidence: 0.20,
        },
    },
    Event {
        tick: 1,
        kind: EventKind::Nominate {
            id: "opposite_pending",
            slot: 0,
            owner: 1,
            polarity: Polarity::Contradiction,
            confidence: 0.85,
        },
    },
    Event {
        tick: 2,
        kind: EventKind::Nominate {
            id: "young_support",
            slot: 0,
            owner: 2,
            polarity: Polarity::Support,
            confidence: 0.95,
        },
    },
    Event {
        tick: 3,
        kind: EventKind::Nominate {
            id: "old_slot_one",
            slot: 1,
            owner: 3,
            polarity: Polarity::Support,
            confidence: 0.40,
        },
    },
    Event {
        tick: 4,
        kind: EventKind::Nominate {
            id: "low_noise",
            slot: 1,
            owner: 4,
            polarity: Polarity::Support,
            confidence: 0.05,
        },
    },
    Event {
        tick: 5,
        kind: EventKind::Nominate {
            id: "young_slot_one",
            slot: 1,
            owner: 5,
            polarity: Polarity::Support,
            confidence: 0.90,
        },
    },
    Event {
        tick: 6,
        kind: EventKind::Nominate {
            id: "polarity_support",
            slot: 2,
            owner: 6,
            polarity: Polarity::Support,
            confidence: 0.70,
        },
    },
    Event {
        tick: 7,
        kind: EventKind::Nominate {
            id: "polarity_contradiction",
            slot: 2,
            owner: 7,
            polarity: Polarity::Contradiction,
            confidence: 0.90,
        },
    },
    Event {
        tick: 8,
        kind: EventKind::Nominate {
            id: "capacity_noise",
            slot: 3,
            owner: 8,
            polarity: Polarity::Support,
            confidence: 0.01,
        },
    },
    Event {
        tick: 9,
        kind: EventKind::Witness {
            slot: 0,
            polarity: Polarity::Support,
            intended: "young_support",
        },
    },
    Event {
        tick: 10,
        kind: EventKind::Witness {
            slot: 1,
            polarity: Polarity::Support,
            intended: "young_slot_one",
        },
    },
    Event {
        tick: 11,
        kind: EventKind::Witness {
            slot: 2,
            polarity: Polarity::Contradiction,
            intended: "polarity_contradiction",
        },
    },
    Event {
        tick: 12,
        kind: EventKind::Nominate {
            id: "opposite_replacement",
            slot: 0,
            owner: 1,
            polarity: Polarity::Support,
            confidence: 0.80,
        },
    },
    Event {
        tick: 13,
        kind: EventKind::Witness {
            slot: 0,
            polarity: Polarity::Contradiction,
            intended: "opposite_pending",
        },
    },
    Event {
        tick: 14,
        kind: EventKind::Witness {
            slot: 0,
            polarity: Polarity::Support,
            intended: "opposite_replacement",
        },
    },
    Event {
        tick: 20,
        kind: EventKind::Witness {
            slot: 1,
            polarity: Polarity::Support,
            intended: "old_slot_one",
        },
    },
    Event {
        tick: 700,
        kind: EventKind::Witness {
            slot: 0,
            polarity: Polarity::Support,
            intended: "old_support",
        },
    },
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Status {
    Live,
    Satisfied,
    Expired(ExpiryCause),
    Evicted,
}

#[derive(Clone, Debug)]
struct Obligation {
    id: &'static str,
    slot: u8,
    owner: u8,
    polarity: Polarity,
    confidence: f32,
    created: u32,
    trace: f32,
    last_tick: u32,
    shadow_expiry: Option<ExpiryCause>,
    status: Status,
    completed_correctly: bool,
}

#[derive(Clone, Debug, Default)]
struct Metrics {
    nominations: u32,
    witnesses: u32,
    updates: u32,
    correct_updates: u32,
    wrong_owner_updates: u32,
    polarity_errors: u32,
    stale_credit_resurrections: u32,
    starvation: u32,
    eligible_witnesses: u32,
    expired_before_witness: u32,
    no_route_witnesses: u32,
    capacity_evictions: u32,
    deadline_expiries: u32,
    superseded_expiries: u32,
    capacity_expiries: u32,
    trace_underflow_events: u32,
    max_unresolved: u32,
    final_unresolved: u32,
}

#[derive(Clone, Debug, Serialize)]
struct ArmReceipt {
    arm: Arm,
    arm_label: &'static str,
    bounded_pending: bool,
    pending_capacity: usize,
    nominations: u32,
    witnesses: u32,
    updates: u32,
    correct_updates: u32,
    wrong_owner_updates: u32,
    polarity_errors: u32,
    stale_credit_resurrections: u32,
    starvation: u32,
    eligible_witnesses: u32,
    expired_before_witness: u32,
    no_route_witnesses: u32,
    capacity_evictions: u32,
    deadline_expiries: u32,
    superseded_expiries: u32,
    capacity_expiries: u32,
    trace_underflow_events: u32,
    max_unresolved: u32,
    final_unresolved: u32,
    unresolved_growth: u32,
}

#[derive(Clone, Debug, Serialize)]
struct ArtifactHashes {
    source_path: String,
    source_sha256: String,
    protocol_path: String,
    protocol_sha256: String,
    executable_path: String,
    executable_sha256: String,
}

#[derive(Clone, Debug, Serialize)]
struct Receipt {
    schema: &'static str,
    protocol: &'static str,
    hypothesis: &'static str,
    semantic_expiry_policy: &'static str,
    lambda: f32,
    minimum_trace: f32,
    deadline_ticks: u32,
    pending_capacity: usize,
    event_count: usize,
    stream_sha256: String,
    artifacts: ArtifactHashes,
    arms: Vec<ArmReceipt>,
    credit_routing_gate: bool,
    gate_reason: &'static str,
    conclusion: &'static str,
}

fn digest_bytes(bytes: impl AsRef<[u8]>) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn stream_hash() -> String {
    let mut hasher = Sha256::new();
    for event in STREAM {
        hasher.update(event.tick.to_le_bytes());
        match event.kind {
            EventKind::Nominate {
                id,
                slot,
                owner,
                polarity,
                confidence,
            } => {
                hasher.update(b"N");
                hasher.update(id.as_bytes());
                hasher.update([slot, owner, polarity as u8]);
                hasher.update(confidence.to_le_bytes());
            }
            EventKind::Witness {
                slot,
                polarity,
                intended,
            } => {
                hasher.update(b"W");
                hasher.update([slot, polarity as u8]);
                hasher.update(intended.as_bytes());
            }
        }
    }
    digest_bytes(hasher.finalize())
}

fn artifact_hashes() -> Result<ArtifactHashes> {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/bin/lt9_la2e1y.rs");
    let protocol = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/LT9_LA2_E1Y_PROTOCOL.md");
    let executable = env::current_exe()?;
    Ok(ArtifactHashes {
        source_path: source.display().to_string(),
        source_sha256: digest_bytes(fs::read(&source)?),
        protocol_path: protocol.display().to_string(),
        protocol_sha256: digest_bytes(fs::read(&protocol)?),
        executable_path: executable.display().to_string(),
        executable_sha256: digest_bytes(fs::read(&executable)?),
    })
}

fn decay(value: f32, ticks: u32) -> f32 {
    value * LAMBDA.powi(i32::try_from(ticks).unwrap_or(i32::MAX))
}

fn increment_expiry(metrics: &mut Metrics, cause: ExpiryCause) {
    match cause {
        ExpiryCause::Deadline => metrics.deadline_expiries += 1,
        ExpiryCause::SupersededByOpposite => metrics.superseded_expiries += 1,
        ExpiryCause::CapacityEviction => metrics.capacity_expiries += 1,
    }
}

fn live_count(obligations: &[Obligation]) -> u32 {
    obligations
        .iter()
        .filter(|obligation| obligation.status == Status::Live)
        .count() as u32
}

fn decay_and_expire(arm: Arm, tick: u32, obligations: &mut [Obligation], metrics: &mut Metrics) {
    for obligation in obligations
        .iter_mut()
        .filter(|item| item.status == Status::Live)
    {
        let delta = tick.saturating_sub(obligation.last_tick);
        if delta > 0 {
            obligation.trace = decay(obligation.trace, delta);
            obligation.last_tick = tick;
        }
        if obligation.trace == 0.0 {
            metrics.trace_underflow_events += 1;
        }
        if tick.saturating_sub(obligation.created) > DEADLINE && obligation.shadow_expiry.is_none()
        {
            obligation.shadow_expiry = Some(ExpiryCause::Deadline);
            if arm.expires() {
                obligation.status = Status::Expired(ExpiryCause::Deadline);
                increment_expiry(metrics, ExpiryCause::Deadline);
            }
        }
    }
}

fn mark_superseded(
    arm: Arm,
    owner: u8,
    slot: u8,
    polarity: Polarity,
    obligations: &mut [Obligation],
    metrics: &mut Metrics,
) {
    for obligation in obligations
        .iter_mut()
        .filter(|item| item.status == Status::Live)
    {
        if obligation.owner == owner
            && obligation.slot == slot
            && obligation.polarity != polarity
            && obligation.shadow_expiry.is_none()
        {
            obligation.shadow_expiry = Some(ExpiryCause::SupersededByOpposite);
            if arm.expires() {
                obligation.status = Status::Expired(ExpiryCause::SupersededByOpposite);
                increment_expiry(metrics, ExpiryCause::SupersededByOpposite);
            }
        }
    }
}

fn evict_if_full(
    arm: Arm,
    new_index: usize,
    obligations: &mut [Obligation],
    metrics: &mut Metrics,
) {
    if !arm.bounded() {
        return;
    }
    let live: Vec<usize> = obligations
        .iter()
        .enumerate()
        .filter_map(|(index, item)| (item.status == Status::Live).then_some(index))
        .collect();
    if live.len() <= PENDING_CAPACITY {
        return;
    }
    let victim = match arm {
        Arm::BinaryPending => live
            .iter()
            .copied()
            .min_by_key(|index| (obligations[*index].created, *index)),
        Arm::PendingConfidence | Arm::PendingConfidenceExpiry => {
            live.iter().copied().min_by(|left, right| {
                obligations[*left]
                    .confidence
                    .total_cmp(&obligations[*right].confidence)
                    .then_with(|| obligations[*left].created.cmp(&obligations[*right].created))
                    .then_with(|| left.cmp(right))
            })
        }
        Arm::TraceOnly => None,
    };
    if let Some(victim) = victim {
        obligations[victim].status = Status::Evicted;
        metrics.capacity_evictions += 1;
        if arm.expires() {
            increment_expiry(metrics, ExpiryCause::CapacityEviction);
        }
    }
    let _ = new_index;
}

fn choose(arm: Arm, slot: u8, polarity: Polarity, obligations: &[Obligation]) -> Option<usize> {
    let mut best: Option<usize> = None;
    for (index, candidate) in obligations.iter().enumerate() {
        if candidate.status != Status::Live || candidate.slot != slot {
            continue;
        }
        if arm == Arm::TraceOnly && candidate.trace <= MIN_TRACE {
            continue;
        }
        if matches!(arm, Arm::PendingConfidence | Arm::PendingConfidenceExpiry)
            && candidate.polarity == polarity
        {
            let replace = best.is_none_or(|old| {
                candidate.confidence > obligations[old].confidence
                    || (candidate.confidence == obligations[old].confidence
                        && candidate.created < obligations[old].created)
            });
            if replace {
                best = Some(index);
            }
        } else if best.is_none() {
            best = Some(index);
        } else if matches!(arm, Arm::TraceOnly) {
            let old = best.expect("best is present");
            if candidate.trace > obligations[old].trace
                || (candidate.trace == obligations[old].trace
                    && candidate.created < obligations[old].created)
            {
                best = Some(index);
            }
        } else if arm == Arm::BinaryPending {
            let old = best.expect("best is present");
            if candidate.created < obligations[old].created {
                best = Some(index);
            }
        } else {
            let old = best.expect("best is present");
            if candidate.confidence > obligations[old].confidence
                || (candidate.confidence == obligations[old].confidence
                    && candidate.created < obligations[old].created)
            {
                best = Some(index);
            }
        }
    }
    if matches!(arm, Arm::PendingConfidence | Arm::PendingConfidenceExpiry) {
        let mut matching = obligations.iter().filter(|item| {
            item.status == Status::Live && item.slot == slot && item.polarity == polarity
        });
        if matching.next().is_none() && arm.expires() {
            return None;
        }
        if obligations.iter().any(|item| {
            item.status == Status::Live && item.slot == slot && item.polarity == polarity
        }) {
            return obligations
                .iter()
                .enumerate()
                .filter(|(_, item)| {
                    item.status == Status::Live && item.slot == slot && item.polarity == polarity
                })
                .max_by(|(_, left), (_, right)| {
                    left.confidence
                        .total_cmp(&right.confidence)
                        .then_with(|| right.created.cmp(&left.created))
                })
                .map(|(index, _)| index);
        }
    }
    best
}

fn run(arm: Arm) -> ArmReceipt {
    let mut obligations = Vec::with_capacity(STREAM.len());
    let mut metrics = Metrics::default();
    for event in STREAM {
        decay_and_expire(arm, event.tick, &mut obligations, &mut metrics);
        match event.kind {
            EventKind::Nominate {
                id,
                slot,
                owner,
                polarity,
                confidence,
            } => {
                mark_superseded(arm, owner, slot, polarity, &mut obligations, &mut metrics);
                let index = obligations.len();
                obligations.push(Obligation {
                    id,
                    slot,
                    owner,
                    polarity,
                    confidence,
                    created: event.tick,
                    trace: 1.0,
                    last_tick: event.tick,
                    shadow_expiry: None,
                    status: Status::Live,
                    completed_correctly: false,
                });
                metrics.nominations += 1;
                evict_if_full(arm, index, &mut obligations, &mut metrics);
            }
            EventKind::Witness {
                slot,
                polarity,
                intended,
            } => {
                metrics.witnesses += 1;
                let intended_index = obligations.iter().position(|item| item.id == intended);
                let intended_live = intended_index.is_some_and(|index| {
                    obligations[index].status == Status::Live
                        && obligations[index].shadow_expiry.is_none()
                });
                let intended_expired = intended_index.is_some_and(|index| {
                    matches!(obligations[index].status, Status::Expired(_))
                        || obligations[index].shadow_expiry.is_some()
                });
                if intended_live {
                    metrics.eligible_witnesses += 1;
                } else if intended_expired {
                    metrics.expired_before_witness += 1;
                }
                let selected = choose(arm, slot, polarity, &obligations);
                let Some(selected) = selected else {
                    metrics.no_route_witnesses += 1;
                    if intended_live {
                        metrics.starvation += 1;
                    }
                    let current = live_count(&obligations);
                    metrics.max_unresolved = metrics.max_unresolved.max(current);
                    continue;
                };
                metrics.updates += 1;
                let stale = obligations[selected].shadow_expiry.is_some();
                if stale {
                    metrics.stale_credit_resurrections += 1;
                }
                if intended_index
                    .is_none_or(|index| obligations[selected].owner != obligations[index].owner)
                {
                    metrics.wrong_owner_updates += 1;
                }
                if obligations[selected].polarity != polarity {
                    metrics.polarity_errors += 1;
                }
                if intended_live && intended_index == Some(selected) {
                    metrics.correct_updates += 1;
                    obligations[selected].completed_correctly = true;
                } else if intended_live {
                    metrics.starvation += 1;
                }
                obligations[selected].status = Status::Satisfied;
            }
        }
        let current = live_count(&obligations);
        metrics.max_unresolved = metrics.max_unresolved.max(current);
    }
    metrics.final_unresolved = live_count(&obligations);
    ArmReceipt {
        arm,
        arm_label: arm.label(),
        bounded_pending: arm.bounded(),
        pending_capacity: if arm.bounded() { PENDING_CAPACITY } else { 0 },
        nominations: metrics.nominations,
        witnesses: metrics.witnesses,
        updates: metrics.updates,
        correct_updates: metrics.correct_updates,
        wrong_owner_updates: metrics.wrong_owner_updates,
        polarity_errors: metrics.polarity_errors,
        stale_credit_resurrections: metrics.stale_credit_resurrections,
        starvation: metrics.starvation,
        eligible_witnesses: metrics.eligible_witnesses,
        expired_before_witness: metrics.expired_before_witness,
        no_route_witnesses: metrics.no_route_witnesses,
        capacity_evictions: metrics.capacity_evictions,
        deadline_expiries: metrics.deadline_expiries,
        superseded_expiries: metrics.superseded_expiries,
        capacity_expiries: metrics.capacity_expiries,
        trace_underflow_events: metrics.trace_underflow_events,
        max_unresolved: metrics.max_unresolved,
        final_unresolved: metrics.final_unresolved,
        unresolved_growth: metrics.max_unresolved,
    }
}

fn build_receipt() -> Result<Receipt> {
    let arms = Arm::ALL.into_iter().map(run).collect::<Vec<_>>();
    let selected = arms
        .iter()
        .find(|arm| arm.arm == Arm::PendingConfidenceExpiry)
        .expect("expiry arm receipt");
    let gate = selected.wrong_owner_updates == 0
        && selected.polarity_errors == 0
        && selected.stale_credit_resurrections == 0
        && selected.starvation == 0
        && selected.max_unresolved <= PENDING_CAPACITY as u32;
    Ok(Receipt {
        schema: SCHEMA,
        protocol: "LT9-LA2-E1Y deterministic overlap, polarity, age, expiry, and bounded-pending assay",
        hypothesis: "pending existence, confidence, and semantic expiry have separable credit-routing failure modes",
        semantic_expiry_policy: "deadline when age > 32 ticks; same-owner opposite-polarity supersession; capacity eviction at seven live entries; expiry is terminal and late witnesses are ignored",
        lambda: LAMBDA,
        minimum_trace: MIN_TRACE,
        deadline_ticks: DEADLINE,
        pending_capacity: PENDING_CAPACITY,
        event_count: STREAM.len(),
        stream_sha256: stream_hash(),
        artifacts: artifact_hashes()?,
        arms,
        credit_routing_gate: gate,
        gate_reason: if gate {
            "zero wrong-owner, polarity, stale-resurrection, and valid-starvation errors within bounded unresolved capacity"
        } else {
            "one or more credit-routing failure metrics remain non-zero"
        },
        conclusion: "E1Y_COMPLETE: diagnostic comparison only; no LA2-B or serving promotion follows",
    })
}

fn main() -> Result<()> {
    let output = env::args()
        .nth(1)
        .unwrap_or_else(|| "D:\\phoenix-evals\\lt9-la2e1y\\lt9-la2e1y-receipt.json".to_owned());
    let receipt = build_receipt()?;
    let bytes = serde_json::to_vec_pretty(&receipt)?;
    let path = Path::new(&output);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, &bytes)?;
    println!(
        "schema={SCHEMA} events={} stream_sha256={} receipt_sha256={}",
        receipt.event_count,
        receipt.stream_sha256,
        digest_bytes(&bytes)
    );
    for arm in &receipt.arms {
        println!("arm={} updates={} correct={} wrong_owner={} polarity={} stale={} starvation={} max_unresolved={} final_unresolved={} evictions={}", arm.arm_label, arm.updates, arm.correct_updates, arm.wrong_owner_updates, arm.polarity_errors, arm.stale_credit_resurrections, arm.starvation, arm.max_unresolved, arm.final_unresolved, arm.capacity_evictions);
    }
    println!("credit_routing_gate={}", receipt.credit_routing_gate);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expiry_blocks_deadline_and_supersession_resurrection() {
        let expiry = run(Arm::PendingConfidenceExpiry);
        let confidence = run(Arm::PendingConfidence);
        assert_eq!(expiry.stale_credit_resurrections, 0);
        assert!(expiry.deadline_expiries > 0);
        assert!(expiry.superseded_expiries > 0);
        assert!(confidence.stale_credit_resurrections > 0);
    }

    #[test]
    fn confidence_reduces_owner_and_polarity_failures_under_overlap() {
        let binary = run(Arm::BinaryPending);
        let confidence = run(Arm::PendingConfidence);
        assert!(binary.wrong_owner_updates > confidence.wrong_owner_updates);
        assert!(binary.polarity_errors > confidence.polarity_errors);
        assert!(confidence.correct_updates > binary.correct_updates);
    }

    #[test]
    fn receipt_replay_is_deterministic_and_capacity_is_bounded() {
        let first = serde_json::to_vec(&build_receipt().expect("build first receipt"))
            .expect("serialize first receipt");
        let second = serde_json::to_vec(&build_receipt().expect("build second receipt"))
            .expect("serialize second receipt");
        assert_eq!(digest_bytes(&first), digest_bytes(&second));
        let expiry = run(Arm::PendingConfidenceExpiry);
        assert!(expiry.max_unresolved <= PENDING_CAPACITY as u32);
        assert!(expiry.capacity_evictions > 0);
    }
}
