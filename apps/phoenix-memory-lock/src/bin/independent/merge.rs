use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use hashbrown::{HashMap, HashSet};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};

const DECISIONS_CONTRACT: &str = "phoenix.qps.relevance-review-decisions/v1";
const RECEIPT_CONTRACT: &str = "phoenix.qps.agent-decision-merge/v1";
const MERGED_REVIEWER: &str = "phoenix-qps-v3-multi-agent-curation-aggregate-v1";
const AUTHORIZATION: &str = "User explicitly authorized continued grouped semantic curation through the QPS V3 promotion corpus and canonical Phase 6-8 chain.";

pub(crate) fn merge(
    cuts_root: &Path,
    packet_path: &Path,
    output_path: &Path,
    receipt_path: &Path,
) -> Result<Publication> {
    refuse_overwrite(output_path)?;
    refuse_overwrite(receipt_path)?;
    let paths = discover_cut_paths(cuts_root)?;
    let evidence = EvidenceIndex::load(packet_path)?;
    let mut decisions = Vec::new();
    let mut identities = BTreeSet::new();
    let mut sources = Vec::with_capacity(paths.len());
    let mut duplicate_evidence_decisions_rejected = 0usize;

    for path in paths {
        let cut: DecisionCut = read_json(&path, "agent decision cut")?;
        validate_cut(&cut)?;
        let submitted = cut.decisions.len();
        let mut admitted = 0usize;
        for decision in cut.decisions {
            validate_decision(&decision)?;
            if !identities.insert(decision.judgment_identity.clone()) {
                bail!("duplicate judgment identity across agent cuts");
            }
            if !evidence.is_first_core(&decision.judgment_identity)? {
                duplicate_evidence_decisions_rejected += 1;
                continue;
            }
            admitted += 1;
            decisions.push(decision);
        }
        let receipt_path = paired_receipt_path(&path)?;
        if !receipt_path.is_file() {
            bail!("missing paired curation receipt {}", receipt_path.display());
        }
        sources.push(SourceCut {
            decisions: file_identity(&path)?,
            receipt: file_identity(&receipt_path)?,
            reviewer_identity: cut.reviewer_identity,
            source_timestamp_present: cut.reviewed_at_unix_seconds != 0,
            submitted_decisions: submitted,
            admitted_decisions: admitted,
        });
    }
    if decisions.is_empty() {
        bail!("agent decision merge has no decisions");
    }

    let reviewed_at_unix_seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before Unix epoch")?
        .as_secs();
    let output = DecisionCut {
        contract: DECISIONS_CONTRACT.to_owned(),
        schema_version: 1,
        reviewer_identity: MERGED_REVIEWER.to_owned(),
        reviewed_at_unix_seconds,
        attestation: "agent_curated_with_user_authorization".to_owned(),
        authorization_context: Some(AUTHORIZATION.to_owned()),
        decisions,
    };
    write_json_create_only(output_path, &output)?;
    let output_file = file_identity(output_path)?;
    let normalized_missing_source_timestamps = sources
        .iter()
        .filter(|source| !source.source_timestamp_present)
        .count();
    let receipt = MergeReceipt {
        contract: RECEIPT_CONTRACT,
        schema_version: 1,
        cuts_root: cuts_root.display().to_string(),
        packet: file_identity(packet_path)?,
        sources,
        decision_count: output.decisions.len(),
        duplicate_judgment_identities: 0,
        duplicate_evidence_decisions_rejected,
        normalized_missing_source_timestamps,
        output: output_file.clone(),
    };
    write_json_create_only(receipt_path, &receipt)?;
    Ok(Publication {
        contract: RECEIPT_CONTRACT,
        output: output_file,
        source_cuts: receipt.sources.len(),
        decision_count: receipt.decision_count,
        duplicate_judgment_identities: 0,
        duplicate_evidence_decisions_rejected,
        normalized_missing_source_timestamps,
    })
}

struct EvidenceIndex {
    first_core_by_judgment: HashMap<String, bool>,
}

impl EvidenceIndex {
    fn load(path: &Path) -> Result<Self> {
        let packet: BundlePacket = read_json(path, "semantic review bundle packet")?;
        Self::from_packet(packet)
    }

    fn from_packet(packet: BundlePacket) -> Result<Self> {
        if packet.bundles.is_empty() {
            bail!("semantic review bundle packet is empty");
        }
        let mut seen_cores = HashSet::with_capacity(packet.bundles.len());
        let mut first_core_by_judgment = HashMap::with_capacity(packet.bundles.len() * 4);
        for bundle in packet.bundles {
            let first = seen_cores.insert(evidence_core(&bundle));
            for challenger in bundle.challengers {
                if !is_sha256(&challenger.judgment_identity)
                    || first_core_by_judgment
                        .insert(challenger.judgment_identity, first)
                        .is_some()
                {
                    bail!("invalid or duplicate judgment in bundle packet");
                }
            }
        }
        Ok(Self {
            first_core_by_judgment,
        })
    }

    fn is_first_core(&self, identity: &str) -> Result<bool> {
        self.first_core_by_judgment
            .get(identity)
            .copied()
            .with_context(|| format!("decision {identity} is absent from bundle packet"))
    }
}

fn evidence_core(bundle: &QueryBundle) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(bundle.dataset.as_bytes());
    hasher.update([0]);
    hasher.update(bundle.reference_answer.trim().as_bytes());
    hasher.update([0]);
    hasher.update(bundle.positive.id.as_bytes());
    hasher.finalize().into()
}

fn discover_cut_paths(root: &Path) -> Result<Vec<PathBuf>> {
    if !root.is_dir() {
        bail!("cuts root is not a directory: {}", root.display());
    }
    let mut paths = fs::read_dir(root)
        .with_context(|| format!("read cuts root {}", root.display()))?
        .map(|entry| entry.map(|value| value.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    paths.retain(|path| {
        path.is_file()
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with("-v1.json") && !name.contains("-receipt"))
    });
    paths.sort_unstable();
    if paths.is_empty() {
        bail!("cuts root contains no *-v1.json decision artifacts");
    }
    Ok(paths)
}

fn paired_receipt_path(cut_path: &Path) -> Result<PathBuf> {
    let name = cut_path
        .file_name()
        .and_then(|value| value.to_str())
        .context("agent cut name is not UTF-8")?;
    let stem = name
        .strip_suffix(".json")
        .context("agent cut does not have a JSON suffix")?;
    Ok(cut_path.with_file_name(format!("{stem}-receipt.json")))
}

fn validate_cut(cut: &DecisionCut) -> Result<()> {
    if cut.contract != DECISIONS_CONTRACT
        || cut.schema_version != 1
        || cut.reviewer_identity.trim().is_empty()
        || cut.attestation != "agent_curated_with_user_authorization"
        || cut
            .authorization_context
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
        || cut.decisions.is_empty()
    {
        bail!("invalid agent decision cut");
    }
    Ok(())
}

fn validate_decision(decision: &ReviewDecision) -> Result<()> {
    if !is_sha256(&decision.judgment_identity)
        || !matches!(
            decision.verdict.as_str(),
            "positive_preferred" | "negative_preferred"
        )
        || decision.reason.trim().is_empty()
        || decision.source != "curated_regression_case"
        || !(0.5..=1.0).contains(&decision.confidence)
        || !decision.confidence.is_finite()
    {
        bail!("invalid agent decision");
    }
    Ok(())
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
}

fn read_json<T: DeserializeOwned>(path: &Path, label: &str) -> Result<T> {
    let bytes = fs::read(path).with_context(|| format!("read {label} {}", path.display()))?;
    decode_json(&bytes).with_context(|| format!("decode {label} {}", path.display()))
}

fn decode_json<T: DeserializeOwned>(bytes: &[u8]) -> serde_json::Result<T> {
    let payload = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    serde_json::from_slice(payload)
}

fn refuse_overwrite(path: &Path) -> Result<()> {
    if path.exists() {
        bail!("refusing to overwrite {}", path.display());
    }
    Ok(())
}

fn write_json_create_only(path: &Path, value: &impl Serialize) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create output directory {}", parent.display()))?;
    }
    let temporary = temporary_path(path);
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .with_context(|| format!("create temporary output {}", temporary.display()))?;
    let mut writer = BufWriter::new(file);
    serde_json::to_writer(&mut writer, value)
        .with_context(|| format!("encode output {}", path.display()))?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    writer.get_ref().sync_all()?;
    fs::rename(&temporary, path)
        .with_context(|| format!("publish create-only output {}", path.display()))?;
    Ok(())
}

fn temporary_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(format!(".{}.tmp", std::process::id()));
    PathBuf::from(name)
}

fn file_identity(path: &Path) -> Result<FileIdentity> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    Ok(FileIdentity {
        path: path.display().to_string(),
        bytes: bytes.len() as u64,
        sha256: hex(&Sha256::digest(&bytes)),
    })
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(DIGITS[(byte >> 4) as usize] as char);
        output.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    output
}

#[derive(Debug, Deserialize, Serialize)]
struct DecisionCut {
    contract: String,
    schema_version: u32,
    reviewer_identity: String,
    #[serde(default)]
    reviewed_at_unix_seconds: u64,
    attestation: String,
    authorization_context: Option<String>,
    decisions: Vec<ReviewDecision>,
}

#[derive(Debug, Deserialize, Serialize)]
struct ReviewDecision {
    judgment_identity: String,
    verdict: String,
    reason: String,
    source: String,
    confidence: f64,
}

#[derive(Clone, Debug, Serialize)]
struct FileIdentity {
    path: String,
    bytes: u64,
    sha256: String,
}

#[derive(Debug, Serialize)]
struct SourceCut {
    decisions: FileIdentity,
    receipt: FileIdentity,
    reviewer_identity: String,
    source_timestamp_present: bool,
    submitted_decisions: usize,
    admitted_decisions: usize,
}

#[derive(Debug, Serialize)]
struct MergeReceipt {
    contract: &'static str,
    schema_version: u32,
    cuts_root: String,
    packet: FileIdentity,
    sources: Vec<SourceCut>,
    decision_count: usize,
    duplicate_judgment_identities: usize,
    duplicate_evidence_decisions_rejected: usize,
    normalized_missing_source_timestamps: usize,
    output: FileIdentity,
}

#[derive(Debug, Serialize)]
pub(crate) struct Publication {
    contract: &'static str,
    output: FileIdentity,
    source_cuts: usize,
    decision_count: usize,
    duplicate_judgment_identities: usize,
    duplicate_evidence_decisions_rejected: usize,
    normalized_missing_source_timestamps: usize,
}

#[derive(Debug, Deserialize)]
struct BundlePacket {
    bundles: Vec<QueryBundle>,
}

#[derive(Debug, Deserialize)]
struct QueryBundle {
    dataset: String,
    #[serde(default)]
    reference_answer: String,
    positive: Document,
    challengers: Vec<Challenger>,
}

#[derive(Debug, Deserialize)]
struct Document {
    id: String,
}

#[derive(Debug, Deserialize)]
struct Challenger {
    judgment_identity: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paired_receipt_name_is_deterministic() {
        let cut = Path::new(r"C:\cuts\scifact-000-029-v1.json");
        assert_eq!(
            paired_receipt_path(cut).expect("paired receipt"),
            PathBuf::from(r"C:\cuts\scifact-000-029-v1-receipt.json")
        );
    }

    #[test]
    fn sha_validation_is_lowercase_and_exact_width() {
        assert!(is_sha256(&"a".repeat(64)));
        assert!(!is_sha256(&"A".repeat(64)));
        assert!(!is_sha256(&"a".repeat(63)));
        assert!(!is_sha256(&"g".repeat(64)));
    }

    #[test]
    fn evidence_index_keeps_only_the_first_packet_core() {
        let bundle = |judgment: usize| QueryBundle {
            dataset: "scifact".to_owned(),
            reference_answer: String::new(),
            positive: Document {
                id: "same-positive".to_owned(),
            },
            challengers: vec![Challenger {
                judgment_identity: format!("{judgment:064x}"),
            }],
        };
        let index = EvidenceIndex::from_packet(BundlePacket {
            bundles: vec![bundle(1), bundle(2)],
        })
        .expect("valid evidence index");
        assert!(index
            .is_first_core(&format!("{:064x}", 1))
            .expect("first judgment"));
        assert!(!index
            .is_first_core(&format!("{:064x}", 2))
            .expect("duplicate judgment"));
    }

    #[test]
    fn json_decoder_accepts_utf8_bom_without_rewriting_source() {
        let value: serde_json::Value =
            decode_json(b"\xef\xbb\xbf{\"contract\":\"test\"}").expect("BOM JSON");
        assert_eq!(value["contract"], "test");
    }
}
