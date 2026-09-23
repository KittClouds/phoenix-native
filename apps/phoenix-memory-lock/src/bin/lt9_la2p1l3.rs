//! LT9-LA2-P1L3: marker-identity attribution and distinct-vote discovery replay.
//!
//! HotpotQA is the frozen discovery corpus. No router, learner, authority,
//! retrieval, or serving artifacts are changed. Expected contexts are used
//! only for post-hoc diagnosis and exact comparison with the LA2-B receipt.

mod lt9_la2p1l3_core;

use anyhow::{ensure, Context, Result};
use lt9_la2p1l3_core::*;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::Path;

const SCHEMA: &str = "phoenix.lexical.lt9-la2p1l3/v1";
const EXPECTED_CORPUS_SHA256: &str = "3e776d2343352f83341878202b8c49cc1ebe6e2ad4c2a77a21c116cafa229334";
const EXPECTED_LA2B_SOURCE_SHA256: &str = "4f51d5a969b09ca05f3a45f25e90fe2ed9f24aa41dfd8cfe39fa8c2883cc5a98";
const EXPECTED_EVENT_COUNT: usize = 2790;
const EXPECTED_EPISODE_COUNT: usize = 2779;
const EXPECTED_LA2B_INVALID_EPISODES: usize = 36;
const EXPECTED_LA2B_INVALID_COMPARTMENTS: usize = 2;

#[derive(Serialize)]
struct MarkerIdentityDetail {
    identity: &'static str,
    bit: u8,
    occurrences: u16,
    before_occurrences: u16,
    between_occurrences: u16,
    after_occurrences: u16,
    repeated_surplus: u16,
}

#[derive(Serialize)]
struct FamilyAttribution {
    family: &'static str,
    raw_occurrences: u16,
    distinct_marker_count: u16,
    distinct_marker_mask: u32,
    repeated_occurrence_surplus: u16,
    active_marker_identities: Vec<MarkerIdentityDetail>,
    raw_winning_family: bool,
    winning_count_gain_identities: Vec<&'static str>,
}

#[derive(Serialize)]
struct EndpointAttribution {
    event_index: usize,
    candidate: &'static str,
    document: u64,
    fixed_priority_family: &'static str,
    raw_unique_argmax_family: Option<&'static str>,
    raw_priority_inversion: bool,
    raw_family_counts: [u16; 3],
    distinct_family_counts: [u16; 3],
    families: [FamilyAttribution; 3],
}

#[derive(Serialize)]
struct EpisodeComparison {
    key: String,
    candidate: &'static str,
    nomination_event_index: usize,
    witness_event_index: usize,
    nomination_document: u64,
    witness_document: u64,
    expected_family: Option<&'static str>,
    raw_route: Option<&'static str>,
    raw_route_path: &'static str,
    distinct_route: Option<&'static str>,
    distinct_route_path: &'static str,
    raw_invalid: Option<bool>,
    distinct_invalid: Option<bool>,
    raw_priority_inversion: bool,
    repeat_surplus_without_new_bit: bool,
    repeated_gain_identities: Vec<&'static str>,
    raw_to_distinct_decision: &'static str,
}

#[derive(Default, Serialize)]
struct ReplayCounts {
    episodes: usize,
    raw_routed: usize,
    distinct_routed: usize,
    raw_abstained: usize,
    distinct_abstained: usize,
    raw_tie_resolved: usize,
    distinct_tie_resolved: usize,
    raw_invalid: usize,
    invalid_repaired_to_correct_route: usize,
    invalid_suppressed_to_abstention: usize,
    invalid_still_wrong: usize,
    valid_baseline: usize,
    valid_lost_to_abstention: usize,
    valid_changed_to_wrong_route: usize,
    valid_preserved: usize,
    newly_created_abstentions: usize,
    previously_abstained_now_routed: usize,
    unlabeled_route_changed: usize,
    all_route_changed: usize,
    repeat_surplus_signatures: usize,
    repeat_surplus_by_candidate: BTreeMap<String, usize>,
    raw_invalid_by_candidate: BTreeMap<String, usize>,
    distinct_invalid_by_candidate: BTreeMap<String, usize>,
}

#[derive(Serialize)]
struct MarkerConcentration {
    identity: &'static str,
    occurrences: u64,
    repeated_surplus: u64,
    priority_inversion_winning_surplus: u64,
}

#[derive(Serialize)]
struct Receipt {
    schema: &'static str,
    scope: &'static str,
    source_corpus: String,
    source_corpus_sha256: String,
    la2b_source_sha256: String,
    document_count: u64,
    event_count: usize,
    episode_count: usize,
    baseline_parity: BaselineParity,
    replay_counts: ReplayCounts,
    baseline_memory: ReplaySummary,
    distinct_vote_memory: ReplaySummary,
    invalid_authority_compartments_prevented: Vec<String>,
    invalid_authority_compartments_introduced: Vec<String>,
    finance_marker_concentration: Vec<MarkerConcentration>,
    endpoint_attribution: Vec<EndpointAttribution>,
    episode_comparisons: Vec<EpisodeComparison>,
    conclusion: &'static str,
}

#[derive(Serialize)]
struct BaselineParity {
    status: &'static str,
    expected_la2b_invalid_routed_episodes: usize,
    replayed_invalid_routed_episodes: usize,
    expected_la2b_invalid_authority_compartments: usize,
    replayed_invalid_authority_compartments: usize,
}

#[derive(Serialize)]
struct CorpusScreenRow {
    corpus_id: String,
    corpus_path: String,
    corpus_sha256: String,
    document_count: u64,
    event_count: usize,
    candidate_relations: usize,
    episode_count: usize,
    routed_episode_count: usize,
    tie_resolved_episode_count: usize,
    priority_inversion_endpoint_count: usize,
    repeat_surplus_without_new_bit_episode_count: usize,
    risk_signature_episode_count: usize,
    risk_signature_candidate_count: usize,
    risk_signature_by_candidate: BTreeMap<String, usize>,
    viability: &'static str,
}

#[derive(Serialize)]
struct CorpusScreenReceipt {
    schema: &'static str,
    scope: &'static str,
    protocol: &'static str,
    qrels_or_validity_labels_opened: bool,
    minimum_risk_signature_episodes: usize,
    minimum_candidate_relations_with_signature: usize,
    preregistered_corpus_order: Vec<String>,
    corpora: Vec<CorpusScreenRow>,
    conclusion: &'static str,
}

fn endpoint_attribution(event_index: usize, event: Event) -> EndpointAttribution {
    let raw = event.features.counts(false);
    let distinct = event.features.counts(true);
    let ties = tie_set(raw);
    let raw_winner = (ties.len() == 1).then_some(ties[0]);
    let priority = fixed_phi(event.features);
    let families = std::array::from_fn(|family_index| {
        let f = event.features.family[family_index];
        let names = marker_names(family_index);
        let active_marker_identities = names.iter().enumerate().filter_map(|(i, name)| {
            let occurrences = f.marker_occurrences[i];
            (occurrences > 0).then_some(MarkerIdentityDetail {
                identity: name,
                bit: i as u8,
                occurrences,
                before_occurrences: f.before_occurrences[i],
                between_occurrences: f.between_occurrences[i],
                after_occurrences: f.after_occurrences[i],
                repeated_surplus: occurrences.saturating_sub(1),
            })
        }).collect::<Vec<_>>();
        let winning_count_gain_identities = if raw_winner == Some(family_index) {
            names.iter().enumerate().filter_map(|(i, name)| (f.marker_occurrences[i] > 1).then_some(*name)).collect()
        } else { Vec::new() };
        FamilyAttribution {
            family: family_name(family_index),
            raw_occurrences: f.raw_count,
            distinct_marker_count: f.distinct_count(),
            distinct_marker_mask: f.distinct_mask,
            repeated_occurrence_surplus: f.repeated_surplus(),
            active_marker_identities,
            raw_winning_family: raw_winner == Some(family_index),
            winning_count_gain_identities,
        }
    });
    EndpointAttribution {
        event_index,
        candidate: CANDIDATES[event.candidate].id,
        document: event.doc,
        fixed_priority_family: family_name(priority),
        raw_unique_argmax_family: raw_winner.map(family_name),
        raw_priority_inversion: raw_winner.is_some_and(|f| f != priority),
        raw_family_counts: raw,
        distinct_family_counts: distinct,
        families,
    }
}

fn unique_winner(f: Features, distinct: bool) -> Option<usize> {
    let set = tie_set(f.counts(distinct));
    (set.len() == 1).then_some(set[0])
}

fn repeated_gain(episode: Episode) -> (bool, Vec<&'static str>, bool) {
    let n = episode.nomination.features;
    let w = episode.witness.features;
    for (winner, other) in [(n, w), (w, n)] {
        let Some(family) = unique_winner(winner, false) else { continue; };
        let other_ties = tie_set(other.counts(false));
        if other_ties.len() != 2 || !other_ties.contains(&family) { continue; }
        if fixed_phi(winner) == family { continue; }
        let wf = winner.family[family];
        let of = other.family[family];
        if wf.distinct_mask != of.distinct_mask || wf.raw_count <= of.raw_count || wf.repeated_surplus() == 0 { continue; }
        let identities = marker_names(family).iter().enumerate()
            .filter_map(|(i, name)| (wf.marker_occurrences[i] > of.marker_occurrences[i]).then_some(*name))
            .collect::<Vec<_>>();
        if !identities.is_empty() { return (true, identities, true); }
    }
    let inversion = [n, w].iter().any(|f| unique_winner(*f, false).is_some_and(|win| fixed_phi(*f) != win));
    (false, Vec::new(), inversion)
}

fn decision_label(raw: Option<usize>, distinct: Option<usize>) -> &'static str {
    match (raw, distinct) {
        (Some(a), Some(b)) if a == b => "SAME_ROUTE",
        (Some(_), Some(_)) => "ROUTE_CHANGED",
        (Some(_), None) => "NEW_ABSTENTION",
        (None, Some(_)) => "NEW_ROUTE",
        (None, None) => "BOTH_ABSTAIN",
    }
}

fn compare_episode(episode: Episode, indices: &BTreeMap<(usize, u64), usize>) -> (EpisodeComparison, bool) {
    let candidate = CANDIDATES[episode.candidate];
    let raw = qualified_pair_route(episode.nomination.features, episode.witness.features, false);
    let distinct = qualified_pair_route(episode.nomination.features, episode.witness.features, true);
    let expected = (candidate.expected_phi >= 0).then_some(candidate.expected_phi as usize);
    let raw_invalid = expected.and_then(|e| raw.map(|r| r != e));
    let distinct_invalid = expected.and_then(|e| distinct.map(|r| r != e));
    let (repeat_signature, repeated_gain_identities, inversion) = repeated_gain(episode);
    let risk_signature = route_path(episode.nomination.features, episode.witness.features, false, raw) == "TIE_RESOLVED"
        && inversion && repeat_signature;
    let comparison = EpisodeComparison {
        key: format!("{}:{}->{}", candidate.id, episode.nomination.doc, episode.witness.doc),
        candidate: candidate.id,
        nomination_event_index: indices[&(episode.candidate, episode.nomination.doc)],
        witness_event_index: indices[&(episode.candidate, episode.witness.doc)],
        nomination_document: episode.nomination.doc,
        witness_document: episode.witness.doc,
        expected_family: expected.map(family_name),
        raw_route: raw.map(family_name),
        raw_route_path: route_path(episode.nomination.features, episode.witness.features, false, raw),
        distinct_route: distinct.map(family_name),
        distinct_route_path: route_path(episode.nomination.features, episode.witness.features, true, distinct),
        raw_invalid,
        distinct_invalid,
        raw_priority_inversion: inversion,
        repeat_surplus_without_new_bit: repeat_signature,
        repeated_gain_identities,
        raw_to_distinct_decision: decision_label(raw, distinct),
    };
    (comparison, risk_signature)
}

fn accumulate_counts(rows: &[EpisodeComparison], signatures: &[bool]) -> ReplayCounts {
    let mut out = ReplayCounts { episodes: rows.len(), ..ReplayCounts::default() };
    for (r, &signature) in rows.iter().zip(signatures) {
        let raw = r.raw_route.map(|name| match name { "finance" => 0, "geography" => 1, "transport" => 2, _ => 3 });
        let distinct = r.distinct_route.map(|name| match name { "finance" => 0, "geography" => 1, "transport" => 2, _ => 3 });
        out.raw_routed += usize::from(raw.is_some());
        out.distinct_routed += usize::from(distinct.is_some());
        out.raw_abstained += usize::from(raw.is_none());
        out.distinct_abstained += usize::from(distinct.is_none());
        out.raw_tie_resolved += usize::from(r.raw_route_path == "TIE_RESOLVED");
        out.distinct_tie_resolved += usize::from(r.distinct_route_path == "TIE_RESOLVED");
        out.all_route_changed += usize::from(raw != distinct);
        out.unlabeled_route_changed += usize::from(r.expected_family.is_none() && raw != distinct);
        out.newly_created_abstentions += usize::from(raw.is_some() && distinct.is_none());
        out.previously_abstained_now_routed += usize::from(raw.is_none() && distinct.is_some());
        if signature {
            out.repeat_surplus_signatures += 1;
            *out.repeat_surplus_by_candidate.entry(r.candidate.to_owned()).or_insert(0) += 1;
        }
        match (r.raw_invalid, r.distinct_invalid, r.expected_family, raw, distinct) {
            (Some(true), Some(false), Some(_), _, Some(_)) => out.invalid_repaired_to_correct_route += 1,
            (Some(true), _, Some(_), Some(_), None) => out.invalid_suppressed_to_abstention += 1,
            (Some(true), Some(true), Some(_), _, Some(_)) => out.invalid_still_wrong += 1,
            (Some(false), Some(false), Some(_), Some(_), Some(_)) => out.valid_preserved += 1,
            (Some(false), _, Some(_), Some(_), None) => out.valid_lost_to_abstention += 1,
            (Some(false), Some(true), Some(_), Some(_), Some(_)) => out.valid_changed_to_wrong_route += 1,
            _ => {}
        }
        out.raw_invalid += usize::from(r.raw_invalid == Some(true));
        if r.raw_invalid == Some(true) { *out.raw_invalid_by_candidate.entry(r.candidate.to_owned()).or_insert(0) += 1; }
        if r.distinct_invalid == Some(true) { *out.distinct_invalid_by_candidate.entry(r.candidate.to_owned()).or_insert(0) += 1; }
    }
    out.valid_baseline = out.valid_preserved + out.valid_lost_to_abstention + out.valid_changed_to_wrong_route;
    out
}

fn finance_concentration(events: &[Event]) -> Vec<MarkerConcentration> {
    let mut raw = vec![0u64; FINANCE.len()];
    let mut surplus = vec![0u64; FINANCE.len()];
    let mut inversion_surplus = vec![0u64; FINANCE.len()];
    for event in events {
        let f = event.features.family[0];
        for i in 0..FINANCE.len() {
            let n = u64::from(f.marker_occurrences[i]);
            raw[i] += n;
            surplus[i] += n.saturating_sub(1);
            if unique_winner(event.features, false) == Some(0) && fixed_phi(event.features) != 0 {
                inversion_surplus[i] += n.saturating_sub(1);
            }
        }
    }
    FINANCE.iter().enumerate().map(|(i, name)| MarkerConcentration {
        identity: name, occurrences: raw[i], repeated_surplus: surplus[i],
        priority_inversion_winning_surplus: inversion_surplus[i],
    }).collect()
}

fn compartment_delta(base: &[String], revised: &[String]) -> (Vec<String>, Vec<String>) {
    let base_set = base.iter().cloned().collect::<std::collections::BTreeSet<_>>();
    let revised_set = revised.iter().cloned().collect::<std::collections::BTreeSet<_>>();
    let prevented = base_set.difference(&revised_set).cloned().collect();
    let introduced = revised_set.difference(&base_set).cloned().collect();
    (prevented, introduced)
}

fn sha256_file(path: &Path) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(std::fs::read(path)?)))
}

fn main() -> Result<()> {
    let args = std::env::args().collect::<Vec<_>>();
    if args.get(1).is_some_and(|s| s == "--screen") {
        return run_corpus_screen(&args[2..]);
    }
    ensure!(args.len() == 4, "usage: lt9_la2p1l3 <hotpotqa-corpus.jsonl> <la2b-source.rs> <output.json>");
    let corpus_path = Path::new(&args[1]);
    let la2b_source_path = Path::new(&args[2]);
    let output_path = Path::new(&args[3]);
    let (events, document_count, corpus_hash) = load_events(corpus_path)?;
    ensure!(corpus_hash == EXPECTED_CORPUS_SHA256, "HotpotQA source hash mismatch: {corpus_hash}");
    let episodes = make_episodes(&events);
    ensure!(events.len() == EXPECTED_EVENT_COUNT, "P1L2/LA2-B event stream drift: {}", events.len());
    ensure!(episodes.len() == EXPECTED_EPISODE_COUNT, "P1L2/LA2-B episode stream drift: {}", episodes.len());
    let la2b_hash = sha256_file(la2b_source_path).context("hash LA2-B source")?;
    ensure!(la2b_hash == EXPECTED_LA2B_SOURCE_SHA256, "LA2-B source hash mismatch: {la2b_hash}");
    let mut indices = BTreeMap::new();
    for (index, event) in events.iter().enumerate() { indices.insert((event.candidate, event.doc), index); }
    ensure!(indices.len() == events.len(), "event identity collision");

    let endpoint_rows = events.iter().enumerate().map(|(i, e)| endpoint_attribution(i, *e)).collect::<Vec<_>>();
    let mut episode_rows = Vec::with_capacity(episodes.len());
    let mut risk_signatures = Vec::with_capacity(episodes.len());
    for episode in episodes.iter().copied() {
        let (row, signature) = compare_episode(episode, &indices);
        episode_rows.push(row);
        risk_signatures.push(signature);
    }
    let replay_counts = accumulate_counts(&episode_rows, &risk_signatures);
    let baseline_memory = run_credit_replay(&episodes, false);
    let distinct_vote_memory = run_credit_replay(&episodes, true);
    ensure!(baseline_memory.invalid_routed_episodes == EXPECTED_LA2B_INVALID_EPISODES,
        "baseline LA2-B parity failure: invalid routed episodes {}", baseline_memory.invalid_routed_episodes);
    ensure!(baseline_memory.invalid_authority_compartments.len() == EXPECTED_LA2B_INVALID_COMPARTMENTS,
        "baseline LA2-B parity failure: invalid authority compartments {}", baseline_memory.invalid_authority_compartments.len());
    let (prevented, introduced) = compartment_delta(
        &baseline_memory.invalid_authority_compartments,
        &distinct_vote_memory.invalid_authority_compartments,
    );
    let parity = BaselineParity {
        status: "MATCHED_LA2B_BASELINE_COUNTS",
        expected_la2b_invalid_routed_episodes: EXPECTED_LA2B_INVALID_EPISODES,
        replayed_invalid_routed_episodes: baseline_memory.invalid_routed_episodes,
        expected_la2b_invalid_authority_compartments: EXPECTED_LA2B_INVALID_COMPARTMENTS,
        replayed_invalid_authority_compartments: baseline_memory.invalid_authority_compartments.len(),
    };
    let receipt = Receipt {
        schema: SCHEMA,
        scope: "P1L3 HotpotQA discovery only; descriptive distinct-marker vote and exact LA2-B credit replay; no qualification, routing update, retrieval, or serving change",
        source_corpus: corpus_path.display().to_string(),
        source_corpus_sha256: corpus_hash,
        la2b_source_sha256: la2b_hash,
        document_count,
        event_count: events.len(),
        episode_count: episodes.len(),
        baseline_parity: parity,
        replay_counts,
        baseline_memory,
        distinct_vote_memory,
        invalid_authority_compartments_prevented: prevented,
        invalid_authority_compartments_introduced: introduced,
        finance_marker_concentration: finance_concentration(&events),
        endpoint_attribution: endpoint_rows,
        episode_comparisons: episode_rows,
        conclusion: "DISCOVERY_ONLY: distinct-marker voting is a counterfactual; HotpotQA is not qualification evidence",
    };
    if let Some(parent) = output_path.parent() { std::fs::create_dir_all(parent)?; }
    std::fs::write(output_path, serde_json::to_vec_pretty(&receipt)?)
        .with_context(|| format!("write {}", output_path.display()))?;
    println!("P1L3 receipt={} events={} episodes={} raw_invalid={} distinct_invalid={} risk_signatures={} compartments_prevented={}",
        output_path.display(), receipt.event_count, receipt.episode_count, receipt.replay_counts.raw_invalid,
        receipt.distinct_vote_memory.invalid_routed_episodes, receipt.replay_counts.repeat_surplus_signatures,
        receipt.invalid_authority_compartments_prevented.len());
    Ok(())
}

fn run_corpus_screen(args: &[String]) -> Result<()> {
    ensure!(args.len() >= 2, "usage: lt9_la2p1l3 --screen <output.json> <corpus-id=corpus.jsonl>...");
    let output_path = Path::new(&args[0]);
    let mut corpora = Vec::with_capacity(args.len() - 1);
    for spec in &args[1..] {
        let (corpus_id, path) = spec.split_once('=').context("screen inputs must be corpus-id=path")?;
        let corpus_path = Path::new(path);
        let (events, document_count, corpus_sha256) = load_events(corpus_path)?;
        let episodes = make_episodes(&events);
        let mut row = CorpusScreenRow {
            corpus_id: corpus_id.to_owned(),
            corpus_path: corpus_path.display().to_string(),
            corpus_sha256,
            document_count,
            event_count: events.len(),
            candidate_relations: events.iter().map(|e| e.candidate).collect::<std::collections::BTreeSet<_>>().len(),
            episode_count: episodes.len(),
            routed_episode_count: 0,
            tie_resolved_episode_count: 0,
            priority_inversion_endpoint_count: 0,
            repeat_surplus_without_new_bit_episode_count: 0,
            risk_signature_episode_count: 0,
            risk_signature_candidate_count: 0,
            risk_signature_by_candidate: BTreeMap::new(),
            viability: "UNDERPOWERED",
        };
        for event in &events {
            let winner = unique_winner(event.features, false);
            if winner.is_some_and(|w| fixed_phi(event.features) != w) {
                row.priority_inversion_endpoint_count += 1;
            }
        }
        for episode in episodes {
            let raw = qualified_pair_route(episode.nomination.features, episode.witness.features, false);
            row.routed_episode_count += usize::from(raw.is_some());
            row.tie_resolved_episode_count += usize::from(route_path(episode.nomination.features, episode.witness.features, false, raw) == "TIE_RESOLVED");
            let (repeat, _, inversion) = repeated_gain(episode);
            row.repeat_surplus_without_new_bit_episode_count += usize::from(repeat);
            let risk = raw.is_some()
                && route_path(episode.nomination.features, episode.witness.features, false, raw) == "TIE_RESOLVED"
                && inversion && repeat;
            if risk {
                row.risk_signature_episode_count += 1;
                *row.risk_signature_by_candidate.entry(CANDIDATES[episode.candidate].id.to_owned()).or_insert(0) += 1;
            }
        }
        row.risk_signature_candidate_count = row.risk_signature_by_candidate.len();
        if row.risk_signature_episode_count >= 8 && row.risk_signature_candidate_count >= 2 {
            row.viability = "RISK_STATE_COVERAGE_FLOOR_MET";
        }
        println!("screened {}: docs={} episodes={} risk_signature={} candidates={} {}",
            row.corpus_id, row.document_count, row.episode_count,
            row.risk_signature_episode_count, row.risk_signature_candidate_count, row.viability);
        corpora.push(row);
    }
    let receipt = CorpusScreenReceipt {
        schema: "phoenix.lexical.lt9-la2p1l3-corpus-screen/v1",
        scope: "label-blind structural suitability only; reads corpus.jsonl text; no qrels, validity judgments, learning, authority, ranking, or serving",
        protocol: "frozen P1L3 risk signature: raw qualified tie-resolved episode plus unique-endpoint priority inversion plus winning-family raw count gain from repeated identity occurrences with identical distinct-marker mask at the tied endpoint; floor >=8 risk episodes across >=2 candidate relations",
        qrels_or_validity_labels_opened: false,
        minimum_risk_signature_episodes: 8,
        minimum_candidate_relations_with_signature: 2,
        preregistered_corpus_order: args[1..].iter().map(|spec| spec.split_once('=').map_or(spec.as_str(), |(id, _)| id).to_owned()).collect(),
        corpora,
        conclusion: "LABEL_BLIND_PREFLIGHT_ONLY: no corpus qualifies the changed router from this receipt",
    };
    if let Some(parent) = output_path.parent() { std::fs::create_dir_all(parent)?; }
    std::fs::write(output_path, serde_json::to_vec_pretty(&receipt)?)?;
    println!("P1L3 corpus preflight receipt: {}", output_path.display());
    Ok(())
}
