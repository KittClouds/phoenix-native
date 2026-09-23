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
            let mut ws = Vec::new();
            push_words(&combined, &mut ws);
            let mut title_words = Vec::new();
            push_words(&title_lower, &mut title_words);
            let title_len = title_words.len();
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
}

#[test]
fn exclusive_only_requires_one_active_family_at_both_endpoints() {
    let exclusive = Features {
        family: [family(2, 0b01), family(0, 0), family(0, 0)],
        ..Features::default()
    };
    let other_exclusive = Features {
        family: [family(0, 0), family(0, 0), family(1, 0b01)],
        ..Features::default()
    };
    let contested = Features {
        family: [family(2, 0b01), family(1, 0b10), family(0, 0)],
        ..Features::default()
    };
    assert_eq!(exclusive_family(exclusive.counts(true)), Some(0));
    assert_eq!(exclusive_family(contested.counts(true)), None);
    assert_eq!(
        qualified_pair_route(exclusive, exclusive, true, TiePolicy::ExclusiveOnly),
        Some(0)
    );
    assert_eq!(
        qualified_pair_route(exclusive, other_exclusive, true, TiePolicy::ExclusiveOnly),
        None
    );
    assert_eq!(
        qualified_pair_route(exclusive, contested, true, TiePolicy::ExclusiveOnly),
        None
    );
}

#[test]
fn exclusive_only_rejects_mixed_endpoints_and_traces_owned_updates() {
    let exclusive = Features {
        family: [family(0, 0), family(0, 0), family(1, 0b01)],
        ..Features::default()
    };
    let contested = Features {
        family: [family(1, 0b01), family(0, 0), family(2, 0b10)],
        ..Features::default()
    };
    assert_eq!(exclusive_family(exclusive.counts(true)), Some(2));
    assert_eq!(exclusive_family(contested.counts(true)), None);
    assert_eq!(
        qualified_pair_route(exclusive, exclusive, true, TiePolicy::ExclusiveOnly),
        Some(2)
    );
    assert_eq!(
        qualified_pair_route(exclusive, contested, true, TiePolicy::ExclusiveOnly),
        None
    );

    let episode = Episode {
        candidate: 1,
        nomination: Event {
            doc: 1,
            candidate: 1,
            features: exclusive,
        },
        witness: Event {
            doc: 2,
            candidate: 1,
            features: exclusive,
        },
    };
    let (summary, trace) = run_credit_replay_traced(&[episode], true, TiePolicy::ExclusiveOnly);
    assert_eq!(summary.plus_updates, 1);
    assert_eq!(summary.owned_witnesses, 1);
    assert_eq!(trace.len(), 1);
    assert_eq!(trace[0].route, Some(2));
    assert_eq!(trace[0].witness_polarity, Some("SUPPORT"));
    assert!(trace[0].owned_witness);
}

#[test]
fn mmap_borrowed_loader_matches_frozen_line_parser() {
    let path =
        std::env::temp_dir().join(format!("p1m1-loader-parity-{}.jsonl", std::process::id()));
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
