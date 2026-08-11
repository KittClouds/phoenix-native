use std::{
    env,
    fs::File,
    io::{BufReader, BufWriter},
    path::PathBuf,
};

use anyhow::{bail, Context, Result};
use hashbrown::HashMap;
use phoenix_lexical_qps::{
    JudgmentReasonV3, LeakageSplitV3, PairwiseJudgmentV3, PrimarySplitV3, RelevanceLedgerV3,
    RANK_EVIDENCE_V3_FEATURE_COUNT, RANK_EVIDENCE_V3_FEATURE_NAMES,
};
use serde::{Deserialize, Serialize};

const PHASE_4_CONTRACT: &str = "phoenix.memory.qps-v3-ledger-qualification/v1";
const PHASE_6_CONTRACT: &str = "phoenix.memory.qps-v3-leakage-split/v1";
const AUDIT_CONTRACT: &str = "phoenix.memory.qps-v3-linear-feasibility/v1";

fn main() -> Result<()> {
    let Args {
        phase_4,
        phase_6,
        output,
    } = Args::parse()?;
    let phase_4: FrozenPhase4 = read_json(&phase_4)?;
    let phase_6: FrozenPhase6 = read_json(&phase_6)?;
    if phase_4.contract != PHASE_4_CONTRACT || !phase_4.phase_4_verified {
        bail!("Phase 4 receipt is not verified");
    }
    if phase_6.contract != PHASE_6_CONTRACT || !phase_6.phase_6_verified {
        bail!("Phase 6 receipt is not verified");
    }

    let receipt = audit(&phase_4.ledger, &phase_6.split)?;
    let writer = BufWriter::new(
        File::create(&output).with_context(|| format!("failed to create {}", output.display()))?,
    );
    serde_json::to_writer_pretty(writer, &receipt)
        .context("failed to write feasibility receipt")?;
    println!(
        "Phase 8 linear feasibility: {} active judgments, {} impossible, {} tie-only",
        receipt.active_judgments,
        receipt.componentwise_negative_dominates,
        receipt.identical_feature_vectors
    );
    println!("Output: {}", output.display());
    Ok(())
}

fn audit(ledger: &RelevanceLedgerV3, split: &LeakageSplitV3) -> Result<FeasibilityReceipt> {
    ledger.validate().map_err(anyhow::Error::msg)?;
    let assignments = split
        .assignments
        .iter()
        .map(|assignment| (assignment.judgment_identity, assignment.primary_split))
        .collect::<HashMap<_, _>>();
    let active = ledger.active_model_training_judgments();
    let mut split_stats = SplitStats::all();
    let mut reason_stats = all_reasons()
        .into_iter()
        .map(ReasonStats::new)
        .collect::<Vec<_>>();
    let mut feature_directions =
        std::array::from_fn(|index| FeatureDirection::new(RANK_EVIDENCE_V3_FEATURE_NAMES[index]));
    let mut missing_assignments = 0_usize;
    let mut positive_dominates = 0_usize;
    let mut negative_dominates = 0_usize;
    let mut identical = 0_usize;
    let mut mixed = 0_usize;
    let mut stable_tie_positive = 0_usize;
    let mut v2_positive = 0_usize;

    for judgment in &active {
        let Some(primary_split) = assignments.get(&judgment.identity).copied() else {
            missing_assignments += 1;
            continue;
        };
        let direction = classify(judgment, &mut feature_directions);
        match direction {
            PairDirection::PositiveDominates => positive_dominates += 1,
            PairDirection::NegativeDominates => negative_dominates += 1,
            PairDirection::Identical => {
                identical += 1;
                stable_tie_positive += usize::from(
                    judgment.positive_document_version < judgment.negative_document_version,
                );
            }
            PairDirection::Mixed => mixed += 1,
        }
        v2_positive += usize::from(judgment.positive_position < judgment.negative_position);
        split_stats.get_mut(primary_split).record(direction);
        reason_stats
            .iter_mut()
            .find(|stats| stats.reason == judgment.reason)
            .expect("all reasons are represented")
            .record(primary_split, direction);
    }

    Ok(FeasibilityReceipt {
        contract: AUDIT_CONTRACT,
        phase_4_contract: phase_4_contract(ledger),
        phase_6_contract: split.contract.clone(),
        active_judgments: active.len(),
        assigned_judgments: active.len() - missing_assignments,
        missing_assignments,
        v2_positive_preferred: v2_positive,
        componentwise_positive_dominates: positive_dominates,
        componentwise_negative_dominates: negative_dominates,
        identical_feature_vectors: identical,
        identical_stable_tie_positive: stable_tie_positive,
        mixed_feature_directions: mixed,
        theoretical_upper_bound_correct: active.len()
            - negative_dominates
            - (identical - stable_tie_positive),
        split_stats,
        reason_stats,
        feature_directions,
    })
}

fn classify(
    judgment: &PairwiseJudgmentV3,
    features: &mut [FeatureDirection; RANK_EVIDENCE_V3_FEATURE_COUNT],
) -> PairDirection {
    let mut positive = false;
    let mut negative = false;
    for (index, (&left, &right)) in judgment
        .positive_features
        .values
        .iter()
        .zip(&judgment.negative_features.values)
        .enumerate()
    {
        match left.total_cmp(&right) {
            std::cmp::Ordering::Greater => {
                positive = true;
                features[index].positive_greater += 1;
            }
            std::cmp::Ordering::Less => {
                negative = true;
                features[index].negative_greater += 1;
            }
            std::cmp::Ordering::Equal => features[index].equal += 1,
        }
    }
    match (positive, negative) {
        (true, false) => PairDirection::PositiveDominates,
        (false, true) => PairDirection::NegativeDominates,
        (false, false) => PairDirection::Identical,
        (true, true) => PairDirection::Mixed,
    }
}

fn phase_4_contract(ledger: &RelevanceLedgerV3) -> String {
    ledger.contract.clone()
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &PathBuf) -> Result<T> {
    let reader = BufReader::new(
        File::open(path).with_context(|| format!("failed to open {}", path.display()))?,
    );
    serde_json::from_reader(reader).with_context(|| format!("failed to parse {}", path.display()))
}

struct Args {
    phase_4: PathBuf,
    phase_6: PathBuf,
    output: PathBuf,
}

impl Args {
    fn parse() -> Result<Self> {
        let mut phase_4 = None;
        let mut phase_6 = None;
        let mut output = None;
        let mut args = env::args().skip(1);
        while let Some(flag) = args.next() {
            let value = args
                .next()
                .with_context(|| format!("missing value for {flag}"))?;
            match flag.as_str() {
                "--phase-4" => phase_4 = Some(PathBuf::from(value)),
                "--phase-6" => phase_6 = Some(PathBuf::from(value)),
                "--output" => output = Some(PathBuf::from(value)),
                _ => bail!("unknown argument {flag}"),
            }
        }
        Ok(Self {
            phase_4: phase_4.context("missing --phase-4")?,
            phase_6: phase_6.context("missing --phase-6")?,
            output: output.context("missing --output")?,
        })
    }
}

#[derive(Debug, Deserialize)]
struct FrozenPhase4 {
    contract: String,
    ledger: RelevanceLedgerV3,
    phase_4_verified: bool,
}

#[derive(Debug, Deserialize)]
struct FrozenPhase6 {
    contract: String,
    split: LeakageSplitV3,
    phase_6_verified: bool,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum PairDirection {
    PositiveDominates,
    NegativeDominates,
    Identical,
    Mixed,
}

#[derive(Debug, Serialize)]
struct FeasibilityReceipt {
    contract: &'static str,
    phase_4_contract: String,
    phase_6_contract: String,
    active_judgments: usize,
    assigned_judgments: usize,
    missing_assignments: usize,
    v2_positive_preferred: usize,
    componentwise_positive_dominates: usize,
    componentwise_negative_dominates: usize,
    identical_feature_vectors: usize,
    identical_stable_tie_positive: usize,
    mixed_feature_directions: usize,
    theoretical_upper_bound_correct: usize,
    split_stats: [SplitStats; 3],
    reason_stats: Vec<ReasonStats>,
    feature_directions: [FeatureDirection; RANK_EVIDENCE_V3_FEATURE_COUNT],
}

#[derive(Clone, Copy, Debug, Serialize)]
struct SplitStats {
    split: PrimarySplitV3,
    judgments: usize,
    positive_dominates: usize,
    negative_dominates: usize,
    identical: usize,
    mixed: usize,
}

impl SplitStats {
    fn all() -> [Self; 3] {
        [
            Self::new(PrimarySplitV3::Training),
            Self::new(PrimarySplitV3::Development),
            Self::new(PrimarySplitV3::BlindTest),
        ]
    }

    const fn new(split: PrimarySplitV3) -> Self {
        Self {
            split,
            judgments: 0,
            positive_dominates: 0,
            negative_dominates: 0,
            identical: 0,
            mixed: 0,
        }
    }

    fn record(&mut self, direction: PairDirection) {
        self.judgments += 1;
        match direction {
            PairDirection::PositiveDominates => self.positive_dominates += 1,
            PairDirection::NegativeDominates => self.negative_dominates += 1,
            PairDirection::Identical => self.identical += 1,
            PairDirection::Mixed => self.mixed += 1,
        }
    }
}

trait SplitStatsExt {
    fn get_mut(&mut self, split: PrimarySplitV3) -> &mut SplitStats;
}

impl SplitStatsExt for [SplitStats; 3] {
    fn get_mut(&mut self, split: PrimarySplitV3) -> &mut SplitStats {
        let index = match split {
            PrimarySplitV3::Training => 0,
            PrimarySplitV3::Development => 1,
            PrimarySplitV3::BlindTest => 2,
        };
        &mut self[index]
    }
}

#[derive(Debug, Serialize)]
struct ReasonStats {
    reason: JudgmentReasonV3,
    total: usize,
    training: usize,
    development: usize,
    blind_test: usize,
    positive_dominates: usize,
    negative_dominates: usize,
    identical: usize,
    mixed: usize,
}

impl ReasonStats {
    const fn new(reason: JudgmentReasonV3) -> Self {
        Self {
            reason,
            total: 0,
            training: 0,
            development: 0,
            blind_test: 0,
            positive_dominates: 0,
            negative_dominates: 0,
            identical: 0,
            mixed: 0,
        }
    }

    fn record(&mut self, split: PrimarySplitV3, direction: PairDirection) {
        self.total += 1;
        match split {
            PrimarySplitV3::Training => self.training += 1,
            PrimarySplitV3::Development => self.development += 1,
            PrimarySplitV3::BlindTest => self.blind_test += 1,
        }
        match direction {
            PairDirection::PositiveDominates => self.positive_dominates += 1,
            PairDirection::NegativeDominates => self.negative_dominates += 1,
            PairDirection::Identical => self.identical += 1,
            PairDirection::Mixed => self.mixed += 1,
        }
    }
}

#[derive(Debug, Serialize)]
struct FeatureDirection {
    feature: &'static str,
    positive_greater: usize,
    negative_greater: usize,
    equal: usize,
}

impl FeatureDirection {
    const fn new(feature: &'static str) -> Self {
        Self {
            feature,
            positive_greater: 0,
            negative_greater: 0,
            equal: 0,
        }
    }
}

const fn all_reasons() -> [JudgmentReasonV3; 12] {
    [
        JudgmentReasonV3::PartialMatchSaturation,
        JudgmentReasonV3::ScatteredTerms,
        JudgmentReasonV3::PhraseOrderFailure,
        JudgmentReasonV3::IdentifierCollision,
        JudgmentReasonV3::FuzzyCollision,
        JudgmentReasonV3::WeakFieldEvidence,
        JudgmentReasonV3::CommonTermDominance,
        JudgmentReasonV3::LengthPriorFailure,
        JudgmentReasonV3::WrongConceptProximity,
        JudgmentReasonV3::DocumentConversationConfusion,
        JudgmentReasonV3::LongQueryFailure,
        JudgmentReasonV3::RealUserCorrection,
    ]
}
