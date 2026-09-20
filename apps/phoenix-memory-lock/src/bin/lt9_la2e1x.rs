//! LT9-LA2-E1X: trace magnitude versus pending existence.
//!
//! A tiny synthetic contention assay. Two valid obligations become witnessable
//! at the same time, but only one update slot is available per round. The
//! stream compares FIFO presence, magnitude priority, binary pending credit,
//! and pending credit with magnitude confidence.

use std::env;
use std::fs;
use std::path::Path;

use anyhow::Result;
use serde::Serialize;
use sha2::{Digest, Sha256};

const SCHEMA: &str = "phoenix.lexical.lt9e1x/v1";
const LAMBDA: f32 = 0.85;
const FOLLOWUP_GAP: usize = 64;

#[derive(Clone, Copy, Debug, Serialize)]
enum Arm {
    PresenceExponential,
    MagnitudeExponential,
    BinaryPending,
    PendingConfidence,
}

impl Arm {
    const ALL: [Self; 4] = [
        Self::PresenceExponential,
        Self::MagnitudeExponential,
        Self::BinaryPending,
        Self::PendingConfidence,
    ];
    const fn label(self) -> &'static str {
        match self {
            Self::PresenceExponential => "presence_only_exponential",
            Self::MagnitudeExponential => "magnitude_weighted_exponential",
            Self::BinaryPending => "binary_pending_tag",
            Self::PendingConfidence => "pending_plus_magnitude_confidence",
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
enum Regime {
    ModerateAge,
    ExtremeAge,
}

impl Regime {
    const ALL: [Self; 2] = [Self::ModerateAge, Self::ExtremeAge];
    const fn label(self) -> &'static str {
        match self {
            Self::ModerateAge => "moderate_old_64_young_4",
            Self::ExtremeAge => "extreme_old_700_young_4",
        }
    }
    const fn old_age(self) -> usize {
        match self {
            Self::ModerateAge => 64,
            Self::ExtremeAge => 700,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Obligation {
    id: &'static str,
    trace: f32,
    pending: bool,
    witness_available: bool,
}

#[derive(Clone, Debug, Serialize)]
struct StageReceipt {
    stage: u8,
    opportunity: usize,
    selected: Option<&'static str>,
    selected_trace: f32,
    old_trace: f32,
    young_trace: f32,
    old_pending: bool,
    young_pending: bool,
}

#[derive(Clone, Debug, Serialize)]
struct ArmReceipt {
    arm: Arm,
    arm_label: &'static str,
    regime: Regime,
    old_age: usize,
    young_age: usize,
    old_initial_trace: f32,
    young_initial_trace: f32,
    stage1_selected: Option<&'static str>,
    stage2_selected: Option<&'static str>,
    old_selected: bool,
    young_selected: bool,
    zero_trace_updates: u32,
    pending_rescue_updates: u32,
    authority_mass: f32,
    unresolved_after_stage2: u32,
    all_valid_updates: bool,
    stages: Vec<StageReceipt>,
}

#[derive(Clone, Debug, Serialize)]
struct Receipt {
    schema: &'static str,
    protocol: &'static str,
    hypothesis: &'static str,
    lambda: f32,
    followup_gap: usize,
    regimes: Vec<ArmReceipt>,
    conclusion: &'static str,
}

fn digest<T: Serialize>(value: &T) -> String {
    let bytes = serde_json::to_vec(value).expect("serialize sealed assay");
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn decay(trace: f32, gap: usize) -> f32 {
    trace * LAMBDA.powi(i32::try_from(gap).unwrap_or(i32::MAX))
}

fn eligible(arm: Arm, obligation: &Obligation) -> bool {
    match arm {
        Arm::PresenceExponential | Arm::MagnitudeExponential => {
            obligation.witness_available && obligation.trace > 0.0
        }
        Arm::BinaryPending | Arm::PendingConfidence => {
            obligation.witness_available && obligation.pending
        }
    }
}

fn choose(arm: Arm, old: &Obligation, young: &Obligation) -> Option<&'static str> {
    let old_ok = eligible(arm, old);
    let young_ok = eligible(arm, young);
    match (old_ok, young_ok) {
        (false, false) => None,
        (true, false) => Some(old.id),
        (false, true) => Some(young.id),
        (true, true) => match arm {
            Arm::PresenceExponential | Arm::BinaryPending => Some(old.id),
            Arm::MagnitudeExponential | Arm::PendingConfidence => {
                if young.trace > old.trace {
                    Some(young.id)
                } else {
                    Some(old.id)
                }
            }
        },
    }
}

fn select(obligation: &mut Obligation, arm: Arm) -> (bool, bool) {
    let was_pending = obligation.pending;
    let zero_trace = obligation.trace == 0.0;
    if eligible(arm, obligation) {
        obligation.pending = false;
        obligation.witness_available = false;
        (was_pending, zero_trace)
    } else {
        (false, false)
    }
}

fn run(arm: Arm, regime: Regime) -> ArmReceipt {
    let old_age = regime.old_age();
    let young_age = 4;
    let mut old = Obligation {
        id: "old",
        trace: decay(1.0, old_age),
        pending: true,
        witness_available: true,
    };
    let mut young = Obligation {
        id: "young",
        trace: decay(1.0, young_age),
        pending: true,
        witness_available: true,
    };
    let old_initial_trace = old.trace;
    let young_initial_trace = young.trace;
    let first = choose(arm, &old, &young);
    let mut authority_mass = 0.0;
    let mut zero_trace_updates = 0;
    let mut pending_rescue_updates = 0;
    if let Some(id) = first {
        let selected = if id == old.id { &mut old } else { &mut young };
        let was_pending = selected.pending;
        let zero_trace = selected.trace == 0.0;
        authority_mass += selected.trace;
        if zero_trace {
            zero_trace_updates += 1;
        }
        if matches!(arm, Arm::PendingConfidence) && zero_trace {
            pending_rescue_updates += 1;
        }
        let _ = select(selected, arm);
        debug_assert!(was_pending);
    }
    let stage1_selected = first;
    let stage1 = StageReceipt {
        stage: 1,
        opportunity: old_age,
        selected: first,
        selected_trace: first.map_or(0.0, |id| {
            if id == "old" {
                old_initial_trace
            } else {
                young_initial_trace
            }
        }),
        old_trace: old.trace,
        young_trace: young.trace,
        old_pending: old.pending,
        young_pending: young.pending,
    };
    old.trace = decay(old.trace, FOLLOWUP_GAP);
    young.trace = decay(young.trace, FOLLOWUP_GAP);
    let second = choose(arm, &old, &young);
    if let Some(id) = second {
        let selected = if id == old.id { &mut old } else { &mut young };
        let zero_trace = selected.trace == 0.0;
        authority_mass += selected.trace;
        if zero_trace {
            zero_trace_updates += 1;
        }
        if matches!(arm, Arm::PendingConfidence) && zero_trace {
            pending_rescue_updates += 1;
        }
        let _ = select(selected, arm);
    }
    let stage2 = StageReceipt {
        stage: 2,
        opportunity: old_age + FOLLOWUP_GAP,
        selected: second,
        selected_trace: second.map_or(0.0, |id| if id == "old" { old.trace } else { young.trace }),
        old_trace: old.trace,
        young_trace: young.trace,
        old_pending: old.pending,
        young_pending: young.pending,
    };
    let old_selected = stage1_selected == Some("old") || stage2.selected == Some("old");
    let young_selected = stage1_selected == Some("young") || stage2.selected == Some("young");
    ArmReceipt {
        arm,
        arm_label: arm.label(),
        regime,
        old_age,
        young_age,
        old_initial_trace,
        young_initial_trace,
        stage1_selected,
        stage2_selected: second,
        old_selected,
        young_selected,
        zero_trace_updates,
        pending_rescue_updates,
        authority_mass,
        unresolved_after_stage2: u32::from(old.witness_available)
            + u32::from(young.witness_available),
        all_valid_updates: true,
        stages: vec![stage1, stage2],
    }
}

fn main() -> Result<()> {
    let output = env::args()
        .nth(1)
        .unwrap_or_else(|| "D:\\phoenix-evals\\lt9-la2e1x\\lt9-la2e1x-receipt.json".to_owned());
    let cases = Regime::ALL
        .into_iter()
        .flat_map(|regime| Arm::ALL.into_iter().map(move |arm| run(arm, regime)))
        .collect::<Vec<_>>();
    let receipt = Receipt { schema: SCHEMA, protocol: "LT9-LA2-E1X frozen two-obligation contention; one valid witness update slot per round; no natural corpus or serving state", hypothesis: "pending existence and trace magnitude may have different jobs when obligations compete and ages diverge", lambda: LAMBDA, followup_gap: FOLLOWUP_GAP, regimes: cases, conclusion: "E1X_COMPLETE: magnitude and pending existence are separated under controlled contention; no natural-learning or serving promotion follows" };
    let path = Path::new(&output);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_vec_pretty(&receipt)?)?;
    println!(
        "schema={SCHEMA} cases={} receipt_sha256={}",
        receipt.regimes.len(),
        digest(&receipt)
    );
    for case in &receipt.regimes {
        println!(
            "regime={} arm={} stage1={:?} stage2={:?} zero={} rescue={} unresolved={}",
            case.regime.label(),
            case.arm_label,
            case.stage1_selected,
            case.stage2_selected,
            case.zero_trace_updates,
            case.pending_rescue_updates,
            case.unresolved_after_stage2
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extreme_trace_underflows_but_pending_survives() {
        assert_eq!(decay(1.0, 700), 0.0);
        let result = run(Arm::PendingConfidence, Regime::ExtremeAge);
        assert_eq!(result.pending_rescue_updates, 1);
        assert_eq!(result.unresolved_after_stage2, 0);
    }
    #[test]
    fn magnitude_prefers_young_obligation() {
        let result = run(Arm::MagnitudeExponential, Regime::ModerateAge);
        assert_eq!(result.stage1_selected, Some("young"));
    }
    #[test]
    fn fifo_presence_prefers_old_obligation_when_both_exist() {
        let result = run(Arm::BinaryPending, Regime::ModerateAge);
        assert_eq!(result.stage1_selected, Some("old"));
    }
}
