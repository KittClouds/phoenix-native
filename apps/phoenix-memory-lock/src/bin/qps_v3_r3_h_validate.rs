//! R3-H blinded judgment validator and authority materializer.
//!
//! The packet and private ledger are the only inputs. Labels 2 and 0 create
//! within-query authority pairs; 1, ?, and unfilled entries remain inert.
//! Unknown ids, duplicate ids, and invalid labels fail closed.

use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Deserialize)]
struct PacketItem {
    packet_id: String,
    #[serde(default)]
    judgment: Option<RawLabel>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawLabel {
    Number(u8),
    Text(String),
}

#[derive(Debug, Deserialize)]
struct LedgerItem {
    packet_id: String,
    dataset: String,
    split: String,
    query_id: String,
    document_id: String,
    stratum: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Label {
    Two,
    One,
    Zero,
    Unknown,
    Unfilled,
}

#[derive(Debug, Serialize)]
struct AuthorityPair {
    dataset: String,
    split: String,
    query_id: String,
    positive_packet_id: String,
    positive_document_id: String,
    negative_packet_id: String,
    negative_document_id: String,
    positive_stratum: String,
    negative_stratum: String,
}

#[derive(Debug, Serialize)]
struct Receipt {
    contract: &'static str,
    status: &'static str,
    packet_sha256: String,
    ledger_sha256: String,
    packets: usize,
    labels_two: usize,
    labels_one: usize,
    labels_zero: usize,
    labels_unknown: usize,
    labels_unfilled: usize,
    queries: usize,
    queries_with_two_and_zero: usize,
    authoritative_pairs: usize,
    pairs_file: String,
}

fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    let packet_path = PathBuf::from(args.next().context(
        "usage: qps_v3_r3_h_validate <packets-json> <ledger-json> <receipt-json> <pairs-json>",
    )?);
    let ledger_path = PathBuf::from(args.next().context("missing ledger json")?);
    let receipt_path = PathBuf::from(args.next().context("missing receipt json")?);
    let pairs_path = PathBuf::from(args.next().context("missing pairs json")?);
    let packet_bytes = fs::read(&packet_path)?;
    let ledger_bytes = fs::read(&ledger_path)?;
    let packets: Vec<PacketItem> = serde_json::from_slice(&packet_bytes)
        .with_context(|| format!("decode {}", packet_path.display()))?;
    let ledger: Vec<LedgerItem> = serde_json::from_slice(&ledger_bytes)
        .with_context(|| format!("decode {}", ledger_path.display()))?;
    validate_ids(&packets, &ledger)?;
    let mut counts = [0_usize; 5];
    let mut labels = HashMap::<String, Label>::with_capacity(packets.len());
    for packet in &packets {
        let label = normalize(packet.judgment.as_ref())?;
        counts[label_index(label)] += 1;
        labels.insert(packet.packet_id.clone(), label);
    }
    let mut query_groups = HashMap::<String, Vec<&LedgerItem>>::new();
    for item in &ledger {
        query_groups
            .entry(format!(
                "{}\0{}\0{}",
                item.dataset, item.split, item.query_id
            ))
            .or_default()
            .push(item);
    }
    let mut pairs = Vec::new();
    let mut queries_with_two_and_zero = 0;
    for items in query_groups.values() {
        let twos = items
            .iter()
            .filter(|item| labels[item.packet_id.as_str()] == Label::Two)
            .collect::<Vec<_>>();
        let zeros = items
            .iter()
            .filter(|item| labels[item.packet_id.as_str()] == Label::Zero)
            .collect::<Vec<_>>();
        if twos.is_empty() || zeros.is_empty() {
            continue;
        }
        queries_with_two_and_zero += 1;
        for positive in &twos {
            for negative in &zeros {
                pairs.push(AuthorityPair {
                    dataset: positive.dataset.clone(),
                    split: positive.split.clone(),
                    query_id: positive.query_id.clone(),
                    positive_packet_id: positive.packet_id.clone(),
                    positive_document_id: positive.document_id.clone(),
                    negative_packet_id: negative.packet_id.clone(),
                    negative_document_id: negative.document_id.clone(),
                    positive_stratum: positive.stratum.clone(),
                    negative_stratum: negative.stratum.clone(),
                });
            }
        }
    }
    let status = if counts[4] > 0 {
        "awaiting_judgments"
    } else if pairs.is_empty() {
        "underpowered_no_authoritative_pairs"
    } else {
        "authority_available"
    };
    let receipt = Receipt {
        contract: "phoenix.qps.r3h-human-authority/v1",
        status,
        packet_sha256: sha256(&packet_bytes),
        ledger_sha256: sha256(&ledger_bytes),
        packets: packets.len(),
        labels_two: counts[0],
        labels_one: counts[1],
        labels_zero: counts[2],
        labels_unknown: counts[3],
        labels_unfilled: counts[4],
        queries: query_groups.len(),
        queries_with_two_and_zero,
        authoritative_pairs: pairs.len(),
        pairs_file: pairs_path.display().to_string(),
    };
    fs::write(&pairs_path, serde_json::to_vec_pretty(&pairs)?)?;
    fs::write(&receipt_path, serde_json::to_vec_pretty(&receipt)?)?;
    println!("{}", serde_json::to_string_pretty(&receipt)?);
    Ok(())
}

fn validate_ids(packets: &[PacketItem], ledger: &[LedgerItem]) -> Result<()> {
    let packet_ids = packets
        .iter()
        .map(|item| item.packet_id.as_str())
        .collect::<HashSet<_>>();
    if packet_ids.len() != packets.len() {
        bail!("duplicate packet id in blinded packet file");
    }
    let ledger_ids = ledger
        .iter()
        .map(|item| item.packet_id.as_str())
        .collect::<HashSet<_>>();
    if ledger_ids.len() != ledger.len() {
        bail!("duplicate packet id in private ledger");
    }
    if packet_ids != ledger_ids {
        bail!("packet and ledger ids differ");
    }
    Ok(())
}

fn normalize(raw: Option<&RawLabel>) -> Result<Label> {
    let Some(raw) = raw else {
        return Ok(Label::Unfilled);
    };
    match raw {
        RawLabel::Number(0) => Ok(Label::Zero),
        RawLabel::Number(1) => Ok(Label::One),
        RawLabel::Number(2) => Ok(Label::Two),
        RawLabel::Number(value) => {
            bail!("invalid numeric judgment label {value}; expected 0, 1, or 2")
        }
        RawLabel::Text(value) if value.trim() == "?" => Ok(Label::Unknown),
        RawLabel::Text(value) if value.trim() == "0" => Ok(Label::Zero),
        RawLabel::Text(value) if value.trim() == "1" => Ok(Label::One),
        RawLabel::Text(value) if value.trim() == "2" => Ok(Label::Two),
        RawLabel::Text(value) => {
            bail!("invalid text judgment label {value:?}; expected 0, 1, 2, or ?")
        }
    }
}

fn label_index(label: Label) -> usize {
    match label {
        Label::Two => 0,
        Label::One => 1,
        Label::Zero => 2,
        Label::Unknown => 3,
        Label::Unfilled => 4,
    }
}

fn sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_accept_only_the_sealed_vocabulary() {
        assert_eq!(normalize(None).unwrap(), Label::Unfilled);
        assert_eq!(normalize(Some(&RawLabel::Number(2))).unwrap(), Label::Two);
        assert_eq!(
            normalize(Some(&RawLabel::Text(" ? ".into()))).unwrap(),
            Label::Unknown
        );
        assert!(normalize(Some(&RawLabel::Number(3))).is_err());
        assert!(normalize(Some(&RawLabel::Text("yes".into()))).is_err());
    }

    #[test]
    fn packet_and_ledger_ids_must_match_exactly() {
        let packets = vec![
            PacketItem {
                packet_id: "a".into(),
                judgment: None,
            },
            PacketItem {
                packet_id: "b".into(),
                judgment: None,
            },
        ];
        let ledger = |id: &str| LedgerItem {
            packet_id: id.into(),
            dataset: "d".into(),
            split: "train".into(),
            query_id: "q".into(),
            document_id: "doc".into(),
            stratum: "control".into(),
        };
        assert!(validate_ids(&packets, &[ledger("a"), ledger("b")]).is_ok());
        assert!(validate_ids(&packets, &[ledger("a"), ledger("a")]).is_err());
        assert!(validate_ids(&packets, &[ledger("a"), ledger("c")]).is_err());
    }
}
