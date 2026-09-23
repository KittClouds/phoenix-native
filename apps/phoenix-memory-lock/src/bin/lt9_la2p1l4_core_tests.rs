
use super::*;
use serde::Deserialize;
use std::io::{BufRead, BufReader, Write};

#[derive(Deserialize)]
struct OwnedCorpusDoc {
    #[serde(default)]
    title: String,
    #[serde(default)]
    text: String,
}

fn reference_line_loader(path: &Path) -> Result<(Vec<Event>, u64, String)> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    let mut hasher = Sha256::new();
    let mut docs = 0u64;
    let mut events = Vec::new();
    while reader.read_line(&mut line)? > 0 {
        hasher.update(line.as_bytes());
        if !line.trim().is_empty() {
            let doc: OwnedCorpusDoc = serde_json::from_str(&line)?;
            let title_lower = doc.title.to_ascii_lowercase();
            let combined = format!("{} {}", title_lower, doc.text.to_ascii_lowercase());
            let ws = words(&combined);
            let title_len = words(&title_lower).len();
            for (candidate, spec) in CANDIDATES.iter().enumerate() {
                if let Some((left, right, distance)) = nearest(&ws, spec.source, spec.target) {
                    if distance <= MAX_PAIR_DISTANCE {
                        events.push(Event {
                            doc: docs,
                            candidate,
                            features: context(&ws, left, right, distance, title_len),
                        });
                    }
                }
            }
            docs += 1;
        }
        line.clear();
    }
    Ok((events, docs, format!("{:x}", hasher.finalize())))
}

fn family(raw: u16, mask: u32) -> FamilyFeatures {
    FamilyFeatures {
        raw_count: raw,
        distinct_mask: mask,
        ..FamilyFeatures::default()
    }
}

#[test]
fn distinct_vote_deduplicates_repeated_identity_only() {
    let repeated = family(3, 0b01);
    assert_eq!(repeated.distinct_count(), 1);
    assert_eq!(repeated.repeated_surplus(), 2);
    assert_eq!(repeated.route_count(false), 3);
    assert_eq!(repeated.route_count(true), 1);
}

#[test]
fn distinct_vote_counterfactual_keeps_tie_resolver_frozen() {
    let a = Features {
        family: [family(2, 1), family(1, 1), family(0, 0)],
        ..Features::default()
    };
    let b = Features {
        family: [family(1, 1), family(1, 1), family(0, 0)],
        ..Features::default()
    };
    assert_eq!(
        qualified_pair_route(a, b, false, TiePolicy::UniqueEndpointAgreement),
        Some(0)
    );
    assert_eq!(
        qualified_pair_route(a, b, true, TiePolicy::UniqueEndpointAgreement),
        None
    );
    assert_eq!(
        qualified_pair_route(a, b, false, TiePolicy::HardAbstain),
        None
    );
}

#[test]
fn mmap_borrowed_loader_matches_frozen_line_parser() {
    let path =
        std::env::temp_dir().join(format!("p1l4-loader-parity-{}.jsonl", std::process::id()));
    let mut file = File::create(&path).expect("create temporary corpus");
    writeln!(file, "{}", serde_json::json!({"_id":"1","title":"BANK and CAR","text":"The river's bank is near the shore. The CAR uses a motor."})).unwrap();
    writeln!(file, "{}", serde_json::json!({"_id":"2","title":"Finance bank","text":"The \"bank\" has a loan; vehicle.\nIt is separate text."})).unwrap();
    writeln!(
        file,
        "{}",
        serde_json::json!({"_id":"3","title":"Café bank","text":"Bank river"})
    )
    .unwrap();
    drop(file);
    let mapped = load_events(&path).expect("mmap corpus");
    let reference = reference_line_loader(&path).expect("reference corpus");
    assert_eq!(
        serde_json::to_vec(&mapped.0).unwrap(),
        serde_json::to_vec(&reference.0).unwrap()
    );
    assert_eq!(mapped.1, reference.1);
    assert_eq!(mapped.2, reference.2);
    std::fs::remove_file(path).unwrap();
}
