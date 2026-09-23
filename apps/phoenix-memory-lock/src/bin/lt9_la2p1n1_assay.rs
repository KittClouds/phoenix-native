use super::core;
use super::core::{Event, FamilyFeatures, Features, CANDIDATES, MAX_MARKERS};
use anyhow::{Context, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;
#[derive(Serialize)]
pub(super) struct CorpusReceipt {
    pub(super) corpus_id: String,
    pub(super) corpus_sha256: String,
    pub(super) document_count: u64,
    pub(super) event_count: usize,
    pub(super) fixed_sense_seed_events: usize,
    pub(super) stream_census: Vec<StreamCensus>,
    pub(super) candidate_token_removal: BTreeMap<String, VariantStats>,
    pub(super) interventions: BTreeMap<String, VariantStats>,
    pub(super) paired_bank_sense: BankSenseStats,
}

#[derive(Serialize)]
pub(super) struct StreamCensus {
    candidate_id: String,
    event_stream_length: usize,
    endpoint_routes: [u64; 3],
    endpoint_abstentions: u64,
    distinct_routed_families: usize,
    shannon_entropy_bits: f64,
    adjacent_unique_pairs: u64,
    phenotype_switches: u64,
    switch_rate: Option<f64>,
    unique_triples: u64,
    aba_triples: u64,
    minority_phenotype_endpoints: u64,
    hard_abstain_pair_routes: u64,
}

#[derive(Clone, Serialize, Default)]
pub(super) struct VariantStats {
    observations: u64,
    baseline_route_counts: [u64; 4],
    variant_route_counts: [u64; 4],
    route_transition_counts: [[u64; 4]; 4],
    stable_route: u64,
    baseline_correct: u64,
    variant_correct: u64,
    abstain_entered: u64,
    abstain_exited: u64,
    injected_marker_identity_sets: BTreeMap<String, u64>,
}

#[derive(Serialize, Default, Clone)]
pub(super) struct BankSenseStats {
    ambient_templates: u64,
    context_only_same_route: u64,
    context_only_distinct_routes: u64,
    all_marker_same_route: u64,
    all_marker_distinct_routes: u64,
    all_marker_shore_correct: u64,
    all_marker_lender_correct: u64,
    all_marker_both_correct: u64,
    context_only_shore_correct: u64,
    context_only_lender_correct: u64,
    context_only_route_counts: [u64; 4],
    route_counts: [[u64; 4]; 4],
}

#[derive(Clone, Copy)]
enum Placement {
    Before,
    Between,
    After,
}

impl Placement {
    fn key(self) -> &'static str {
        match self {
            Self::Before => "before",
            Self::Between => "between",
            Self::After => "after",
        }
    }
}

pub(super) fn sha256_file(path: &Path) -> Result<String> {
    let mut file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut hasher = Sha256::new();
    // Keep streaming hashes well below the Windows main-thread stack limit.
    let mut block = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut block)?;
        if read == 0 {
            break;
        }
        hasher.update(&block[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn route(features: Features, raw: bool) -> Option<usize> {
    let counts = features.counts(!raw);
    let maximum = counts.iter().copied().max()?;
    let mut winners = counts
        .iter()
        .enumerate()
        .filter(|(_, count)| **count == maximum);
    let winner = winners.next()?.0;
    winners.next().is_none().then_some(winner)
}

fn route_index(route: Option<usize>) -> usize {
    route.unwrap_or(3)
}

pub(super) fn declared_family(candidate_id: &str) -> Option<usize> {
    match candidate_id {
        "car_to_vehicle" | "vehicle_to_car" | "engine_to_motor" => Some(2),
        "insurance_to_coverage"
        | "credit_to_loan"
        | "loan_to_debt"
        | "stock_to_bond"
        | "bank_to_lender" => Some(0),
        "bank_to_shore" => Some(1),
        _ => None,
    }
}

fn marker_location_mut(family: &mut FamilyFeatures, place: Placement) -> &mut [u16; MAX_MARKERS] {
    match place {
        Placement::Before => &mut family.before_occurrences,
        Placement::Between => &mut family.between_occurrences,
        Placement::After => &mut family.after_occurrences,
    }
}

fn add_marker(
    features: &mut Features,
    family_index: usize,
    marker_index: usize,
    multiplicity: u16,
    place: Placement,
) {
    let family = &mut features.family[family_index];
    let location = marker_location_mut(family, place);
    location[marker_index] = location[marker_index].saturating_add(multiplicity);
    family.marker_occurrences[marker_index] =
        family.marker_occurrences[marker_index].saturating_add(multiplicity);
    family.raw_count = family.raw_count.saturating_add(multiplicity);
    family.distinct_mask |= 1u32 << marker_index;
}

fn remove_word_vote(features: &mut Features, word: &str) {
    for family_index in 0..3 {
        if let Some(marker_index) = core::marker_names(family_index)
            .iter()
            .position(|marker| marker.eq_ignore_ascii_case(word))
        {
            let family = &mut features.family[family_index];
            if family.marker_occurrences[marker_index] == 0 {
                return;
            }
            family.marker_occurrences[marker_index] -= 1;
            family.raw_count = family.raw_count.saturating_sub(1);
            family.between_occurrences[marker_index] =
                family.between_occurrences[marker_index].saturating_sub(1);
            if family.marker_occurrences[marker_index] == 0 {
                family.distinct_mask &= !(1u32 << marker_index);
            }
            return;
        }
    }
}

fn without_candidate_votes(mut features: Features, candidate: &core::Candidate) -> Features {
    remove_word_vote(&mut features, candidate.source);
    remove_word_vote(&mut features, candidate.target);
    features
}

fn candidate_words(candidate: &core::Candidate, marker: &str) -> bool {
    candidate.source.eq_ignore_ascii_case(marker) || candidate.target.eq_ignore_ascii_case(marker)
}

fn available_markers(
    features: Features,
    candidate: &core::Candidate,
    family_index: usize,
    count: usize,
) -> Vec<usize> {
    core::marker_names(family_index)
        .iter()
        .enumerate()
        .filter(|(index, marker)| {
            !candidate_words(candidate, marker)
                && features.family[family_index].distinct_mask & (1u32 << index) == 0
        })
        .map(|(index, _)| index)
        .take(count)
        .collect()
}

fn add_words_for_candidate(features: &mut Features, candidate: &core::Candidate, place: Placement) {
    for word in [candidate.source, candidate.target] {
        for family_index in 0..3 {
            if let Some(marker_index) = core::marker_names(family_index)
                .iter()
                .position(|marker| marker.eq_ignore_ascii_case(word))
            {
                add_marker(features, family_index, marker_index, 1, place);
                break;
            }
        }
    }
}

fn bucket_only(features: Features, place: Placement) -> Features {
    let mut output = Features::default();
    for family_index in 0..3 {
        let source = &features.family[family_index];
        let target = &mut output.family[family_index];
        let counts = match place {
            Placement::Before => source.before_occurrences,
            Placement::Between => source.between_occurrences,
            Placement::After => source.after_occurrences,
        };
        target.marker_occurrences = counts;
        target.raw_count = counts.iter().copied().sum();
        target.distinct_mask = counts.iter().enumerate().fold(0u32, |mask, (i, count)| {
            if *count > 0 {
                mask | (1u32 << i)
            } else {
                mask
            }
        });
        match place {
            Placement::Before => target.before_occurrences = counts,
            Placement::Between => target.between_occurrences = counts,
            Placement::After => target.after_occurrences = counts,
        }
    }
    output
}

fn record_variant(
    stats: &mut VariantStats,
    baseline: Option<usize>,
    variant: Option<usize>,
    expected: usize,
) {
    let a = route_index(baseline);
    let b = route_index(variant);
    stats.observations += 1;
    stats.baseline_route_counts[a] += 1;
    stats.variant_route_counts[b] += 1;
    stats.route_transition_counts[a][b] += 1;
    stats.stable_route += u64::from(a == b);
    stats.baseline_correct += u64::from(baseline == Some(expected));
    stats.variant_correct += u64::from(variant == Some(expected));
    stats.abstain_entered += u64::from(baseline.is_some() && variant.is_none());
    stats.abstain_exited += u64::from(baseline.is_none() && variant.is_some());
}

fn note_marker_set(stats: &mut VariantStats, family: usize, indices: &[usize]) {
    let names = indices
        .iter()
        .map(|index| core::marker_names(family)[*index])
        .collect::<Vec<_>>()
        .join("+");
    *stats
        .injected_marker_identity_sets
        .entry(names)
        .or_default() += 1;
}
fn intervention_name(
    relation: &str,
    base: &str,
    foreign_family: usize,
    count: usize,
    place: Placement,
) -> String {
    format!(
        "{relation}/{base}/add-{}/n{count}/{}",
        core::family_name(foreign_family),
        place.key()
    )
}

pub(super) fn stream_census(
    candidate: usize,
    stream: &[Event],
    pair_routes: &[Option<usize>],
) -> StreamCensus {
    let mut routes = [0u64; 3];
    let mut abstentions = 0u64;
    let mut route_seq = Vec::with_capacity(stream.len());
    for event in stream {
        let assigned = route(event.features, false);
        if let Some(family) = assigned {
            routes[family] += 1;
        } else {
            abstentions += 1;
        }
        route_seq.push(assigned);
    }
    let routed = routes.iter().sum::<u64>();
    let distinct = routes.iter().filter(|count| **count > 0).count();
    let entropy = if routed == 0 {
        0.0
    } else {
        routes
            .iter()
            .filter(|count| **count > 0)
            .map(|count| {
                let p = *count as f64 / routed as f64;
                -p * p.log2()
            })
            .sum()
    };
    let mut adjacent_unique = 0u64;
    let mut switches = 0u64;
    for pair in route_seq.windows(2) {
        if let [Some(a), Some(b)] = pair {
            adjacent_unique += 1;
            switches += u64::from(a != b);
        }
    }
    let mut triples = 0u64;
    let mut aba = 0u64;
    for triple in route_seq.windows(3) {
        if let [Some(a), Some(b), Some(c)] = triple {
            triples += 1;
            aba += u64::from(a == c && a != b);
        }
    }
    let minority = routed.saturating_sub(routes.iter().copied().max().unwrap_or(0));
    StreamCensus {
        candidate_id: CANDIDATES[candidate].id.to_string(),
        event_stream_length: stream.len(),
        endpoint_routes: routes,
        endpoint_abstentions: abstentions,
        distinct_routed_families: distinct,
        shannon_entropy_bits: entropy,
        adjacent_unique_pairs: adjacent_unique,
        phenotype_switches: switches,
        switch_rate: (adjacent_unique > 0).then_some(switches as f64 / adjacent_unique as f64),
        unique_triples: triples,
        aba_triples: aba,
        minority_phenotype_endpoints: minority,
        hard_abstain_pair_routes: pair_routes.iter().filter(|route| route.is_none()).count() as u64,
    }
}

pub(super) fn analyze_event(
    event: Event,
    interventions: &mut BTreeMap<String, VariantStats>,
    candidate_removal: &mut BTreeMap<String, VariantStats>,
    bank_sense: &mut BankSenseStats,
) {
    let candidate = &CANDIDATES[event.candidate];
    if let Some(expected) = declared_family(candidate.id) {
        let base = event.features;
        let context_only = without_candidate_votes(base, candidate);
        let all_route = route(base, false);
        let context_route = route(context_only, false);
        record_variant(
            candidate_removal
                .entry(format!("{}/candidate-votes-removed", candidate.id))
                .or_default(),
            all_route,
            context_route,
            expected,
        );

        for place in [Placement::Before, Placement::Between, Placement::After] {
            let name = format!("{}/bucket-only/{}", candidate.id, place.key());
            let bucket = bucket_only(base, place);
            record_variant(
                interventions.entry(name).or_default(),
                all_route,
                route(bucket, false),
                expected,
            );
        }

        for (base_name, base_features, baseline_route) in [
            ("all", base, all_route),
            ("context-only", context_only, context_route),
        ] {
            for foreign_family in 0..3 {
                if foreign_family == expected {
                    continue;
                }
                for marker_count in [1usize, 3] {
                    let selected =
                        available_markers(base_features, candidate, foreign_family, marker_count);
                    if selected.len() != marker_count {
                        continue;
                    }
                    for place in [Placement::Before, Placement::Between, Placement::After] {
                        let mut variant = base_features;
                        for marker in &selected {
                            add_marker(&mut variant, foreign_family, *marker, 1, place);
                        }
                        let name = intervention_name(
                            candidate.id,
                            base_name,
                            foreign_family,
                            marker_count,
                            place,
                        );
                        let stats = interventions.entry(name).or_default();
                        record_variant(stats, baseline_route, route(variant, false), expected);
                        note_marker_set(stats, foreign_family, &selected);
                        if marker_count == 1 {
                            let nearby = format!(
                                "{}/{base_name}/nearby-{}/{}",
                                candidate.id,
                                core::family_name(foreign_family),
                                place.key()
                            );
                            let stats = interventions.entry(nearby).or_default();
                            record_variant(stats, baseline_route, route(variant, false), expected);
                            note_marker_set(stats, foreign_family, &selected);
                        }
                    }
                    if marker_count == 1 {
                        let selected_marker = selected[0];
                        for multiplicity in [1u16, 2, 4] {
                            let mut variant = base_features;
                            add_marker(
                                &mut variant,
                                foreign_family,
                                selected_marker,
                                multiplicity,
                                Placement::Before,
                            );
                            let stem = format!(
                                "{}/{base_name}/multiplicity-{}/m{multiplicity}",
                                candidate.id,
                                core::family_name(foreign_family)
                            );
                            let distinct_stats = interventions
                                .entry(format!("{stem}/distinct-vote"))
                                .or_default();
                            record_variant(
                                distinct_stats,
                                baseline_route,
                                route(variant, false),
                                expected,
                            );
                            note_marker_set(distinct_stats, foreign_family, &[selected_marker]);
                            let raw_stats = interventions
                                .entry(format!("{stem}/raw-count-diagnostic"))
                                .or_default();
                            record_variant(
                                raw_stats,
                                route(base_features, true),
                                route(variant, true),
                                expected,
                            );
                            note_marker_set(raw_stats, foreign_family, &[selected_marker]);
                        }
                        let distant = format!(
                            "{}/{base_name}/distant-outside-window-{}",
                            candidate.id,
                            core::family_name(foreign_family)
                        );
                        record_variant(
                            interventions.entry(distant).or_default(),
                            baseline_route,
                            baseline_route,
                            expected,
                        );
                    }
                }
            }
        }
    } else if candidate.id == "bank_to_water" {
        let ambient = without_candidate_votes(event.features, candidate);
        let shore = CANDIDATES
            .iter()
            .find(|item| item.id == "bank_to_shore")
            .unwrap();
        let lender = CANDIDATES
            .iter()
            .find(|item| item.id == "bank_to_lender")
            .unwrap();
        let mut shore_all = ambient;
        let mut lender_all = ambient;
        add_words_for_candidate(&mut shore_all, shore, Placement::Between);
        add_words_for_candidate(&mut lender_all, lender, Placement::Between);
        let shore_all_route = route(shore_all, false);
        let lender_all_route = route(lender_all, false);
        let shore_context_route = route(ambient, false);
        let lender_context_route = route(ambient, false);

        bank_sense.ambient_templates += 1;
        bank_sense.context_only_same_route +=
            u64::from(shore_context_route == lender_context_route);
        bank_sense.context_only_distinct_routes +=
            u64::from(shore_context_route != lender_context_route);
        bank_sense.all_marker_same_route += u64::from(shore_all_route == lender_all_route);
        bank_sense.all_marker_distinct_routes += u64::from(shore_all_route != lender_all_route);
        bank_sense.all_marker_shore_correct += u64::from(shore_all_route == Some(1));
        bank_sense.all_marker_lender_correct += u64::from(lender_all_route == Some(0));
        bank_sense.all_marker_both_correct +=
            u64::from(shore_all_route == Some(1) && lender_all_route == Some(0));
        bank_sense.context_only_shore_correct += u64::from(shore_context_route == Some(1));
        bank_sense.context_only_lender_correct += u64::from(lender_context_route == Some(0));
        bank_sense.context_only_route_counts[route_index(shore_context_route)] += 1;
        bank_sense.route_counts[route_index(shore_all_route)][route_index(lender_all_route)] += 1;
    }
}

pub(super) fn merge_stats(target: &mut VariantStats, source: &VariantStats) {
    target.observations += source.observations;
    for i in 0..4 {
        target.baseline_route_counts[i] += source.baseline_route_counts[i];
        target.variant_route_counts[i] += source.variant_route_counts[i];
        for j in 0..4 {
            target.route_transition_counts[i][j] += source.route_transition_counts[i][j];
        }
    }
    target.stable_route += source.stable_route;
    target.baseline_correct += source.baseline_correct;
    target.variant_correct += source.variant_correct;
    target.abstain_entered += source.abstain_entered;
    target.abstain_exited += source.abstain_exited;
    for (marker_set, count) in &source.injected_marker_identity_sets {
        *target
            .injected_marker_identity_sets
            .entry(marker_set.clone())
            .or_default() += count;
    }
}

pub(super) fn merge_bank_sense(target: &mut BankSenseStats, source: &BankSenseStats) {
    target.ambient_templates += source.ambient_templates;
    target.context_only_same_route += source.context_only_same_route;
    target.context_only_distinct_routes += source.context_only_distinct_routes;
    target.all_marker_same_route += source.all_marker_same_route;
    target.all_marker_distinct_routes += source.all_marker_distinct_routes;
    target.all_marker_shore_correct += source.all_marker_shore_correct;
    target.all_marker_lender_correct += source.all_marker_lender_correct;
    target.all_marker_both_correct += source.all_marker_both_correct;
    target.context_only_shore_correct += source.context_only_shore_correct;
    target.context_only_lender_correct += source.context_only_lender_correct;
    for i in 0..4 {
        target.context_only_route_counts[i] += source.context_only_route_counts[i];
        for j in 0..4 {
            target.route_counts[i][j] += source.route_counts[i][j];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidate_vote_removal_subtracts_only_one_occurrence_per_endpoint() {
        let candidate = CANDIDATES
            .iter()
            .find(|c| c.id == "car_to_vehicle")
            .unwrap();
        let mut features = Features::default();
        add_marker(&mut features, 2, 0, 2, Placement::Between);
        add_marker(&mut features, 2, 1, 1, Placement::Between);
        let context_only = without_candidate_votes(features, candidate);
        assert_eq!(context_only.family[2].marker_occurrences[0], 1);
        assert_eq!(context_only.family[2].marker_occurrences[1], 0);
        assert_eq!(context_only.family[2].distinct_count(), 1);
    }

    #[test]
    fn repeated_marker_identity_changes_raw_not_distinct_vote() {
        let mut features = Features::default();
        add_marker(&mut features, 0, 0, 4, Placement::Before);
        assert_eq!(features.family[0].raw_count, 4);
        assert_eq!(features.family[0].distinct_count(), 1);
        assert_eq!(route(features, false), Some(0));
        assert_eq!(route(features, true), Some(0));
    }

    #[test]
    fn exact_plurality_ties_abstain_including_empty_features() {
        assert_eq!(route(Features::default(), false), None);
        let mut features = Features::default();
        add_marker(&mut features, 0, 0, 1, Placement::Before);
        add_marker(&mut features, 1, 0, 1, Placement::After);
        assert_eq!(route(features, false), None);
    }

    #[test]
    fn streaming_hash_matches_across_multiple_blocks() {
        let path =
            std::env::temp_dir().join(format!("lt9-la2-p1n1-hash-test-{}.bin", std::process::id()));
        let payload = vec![b'x'; 160_000];
        std::fs::write(&path, &payload).unwrap();
        let actual = sha256_file(&path).unwrap();
        let expected = format!("{:x}", Sha256::digest(&payload));
        let _ = std::fs::remove_file(&path);
        assert_eq!(actual, expected);
    }
}
