use super::core::{marker_names, CANDIDATES, NEGATIVE, SUPPORT};
use super::*;
use std::collections::BTreeSet;
pub(super) fn relation(candidate: usize) -> &'static Relation {
    RELATIONS
        .iter()
        .find(|r| r.candidate_index == candidate)
        .expect("frozen relation candidate index")
}

pub(super) fn candidate_words(candidate: usize) -> (&'static str, &'static str) {
    let spec = &CANDIDATES[candidate];
    (spec.source, spec.target)
}

pub(super) fn normalize(word: &str) -> String {
    word.to_ascii_lowercase()
}

pub(super) fn topic_family(r: &Relation, local_frame: usize, opposing: bool) -> usize {
    if opposing {
        if local_frame == r.frame_a {
            r.frame_b
        } else {
            r.frame_a
        }
    } else {
        (0..3)
            .find(|f| *f != r.frame_a && *f != r.frame_b)
            .expect("each pair has a third ambient family")
    }
}

pub(super) fn local_cues(r: &Relation, frame: usize, group: u8) -> Vec<&'static str> {
    let cues = if frame == r.frame_a {
        r.cues_a
    } else {
        r.cues_b
    };
    let start = usize::from(group) % cues.len();
    (0..4)
        .map(|offset| cues[(start + offset * 3) % cues.len()])
        .collect()
}

pub(super) fn ambient_markers(
    r: &Relation,
    family: usize,
    local: &[&str],
    count: usize,
    group: u8,
) -> Vec<String> {
    let (source, target) = candidate_words(r.candidate_index);
    let markers = marker_names(family);
    let mut out = Vec::with_capacity(count);
    let start = usize::from(group) % markers.len();
    for offset in 0..markers.len() {
        let marker = markers[(start + offset) % markers.len()];
        if marker.eq_ignore_ascii_case(source)
            || marker.eq_ignore_ascii_case(target)
            || local.iter().any(|cue| cue.eq_ignore_ascii_case(marker))
            || out
                .iter()
                .any(|prior: &String| prior.eq_ignore_ascii_case(marker))
        {
            continue;
        }
        out.push(marker.to_ascii_lowercase());
        if out.len() == count {
            break;
        }
    }
    out
}

pub(super) fn emit_variant(
    candidate: usize,
    group: u8,
    condition: Condition,
    frame: Option<usize>,
    ambient_family: Option<usize>,
    ambient_count: usize,
) -> Stimulus {
    let r = relation(candidate);
    let (source, target) = candidate_words(candidate);
    let mut tokens = NEUTRAL_WORDS
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
    tokens.resize(16, "plain".to_string());
    tokens[SOURCE_POS] = normalize(source);
    tokens[TARGET_POS] = normalize(target);
    let cues = frame.map(|f| local_cues(r, f, group)).unwrap_or_default();
    for (slot, cue) in SLOT_POSITIONS.iter().copied().zip(cues.iter().copied()) {
        tokens[slot] = normalize(cue);
    }
    if condition == Condition::Conflict {
        tokens[2] = normalize(r.cues_a[usize::from(group) % r.cues_a.len()]);
        tokens[4] = normalize(r.cues_b[usize::from(group) % r.cues_b.len()]);
        tokens[6] = "unlike".to_string();
        tokens[8] = normalize(r.cues_a[(usize::from(group) + 3) % r.cues_a.len()]);
        tokens[9] = normalize(r.cues_b[(usize::from(group) + 3) % r.cues_b.len()]);
    }
    if condition != Condition::CueRemoved && condition != Condition::Conflict && group % 2 == 0 {
        tokens[3] = "also".to_string();
    }
    if let Some(family) = ambient_family {
        let cue_words = cues.iter().copied().collect::<Vec<_>>();
        let ambient = ambient_markers(r, family, &cue_words, ambient_count, group);
        for (slot, marker) in (11..16).zip(ambient) {
            tokens[slot] = marker;
        }
    }
    let label = if matches!(condition, Condition::CueRemoved | Condition::Conflict) {
        None
    } else {
        frame
    };
    // Candidate field placement is balanced and independent of the frame.
    let title_end = match group % 4 {
        0 => 11,
        1 => 6,
        2 => 9,
        _ => 12,
    };
    Stimulus {
        candidate,
        group,
        condition,
        label,
        tokens,
        title_end,
    }
}

pub(super) fn generate_stimuli() -> Vec<Stimulus> {
    let mut all = Vec::with_capacity(RELATIONS.len() * usize::from(GROUPS) * 8);
    for r in &RELATIONS {
        for group in 0..GROUPS {
            for (condition, frame, opposing, count) in [
                (Condition::ABase, Some(r.frame_a), false, 1),
                (Condition::ATopicSwap, Some(r.frame_a), true, 3),
                (Condition::ATopicAmplify, Some(r.frame_a), true, 5),
                (Condition::BBase, Some(r.frame_b), false, 1),
                (Condition::BTopicSwap, Some(r.frame_b), true, 3),
                (Condition::BTopicAmplify, Some(r.frame_b), true, 5),
                (Condition::CueRemoved, None, false, 1),
                (Condition::Conflict, Some(r.frame_a), false, 1),
            ] {
                let family =
                    if condition == Condition::CueRemoved || condition == Condition::Conflict {
                        Some(topic_family(r, r.frame_a, false))
                    } else {
                        Some(topic_family(r, frame.expect("known frame"), opposing))
                    };
                let effective_count = if condition == Condition::Conflict {
                    1
                } else {
                    count
                };
                all.push(emit_variant(
                    r.candidate_index,
                    group,
                    condition,
                    frame,
                    family,
                    effective_count,
                ));
            }
        }
    }
    all
}

pub(super) fn local_features(stimulus: &Stimulus) -> FeatureMap {
    let r = relation(stimulus.candidate);
    let (source, target) = candidate_words(stimulus.candidate);
    let mut features = FeatureMap::default();
    let eligible = |i: usize, token: &str| {
        i != SOURCE_POS
            && i != TARGET_POS
            && !token.eq_ignore_ascii_case(source)
            && !token.eq_ignore_ascii_case(target)
    };
    let role = |i: usize| {
        if i < SOURCE_POS {
            "before"
        } else if i < TARGET_POS {
            "between"
        } else {
            "after"
        }
    };
    let dist_bucket = |d: usize| {
        if d <= 1 {
            "1"
        } else if d <= 2 {
            "2"
        } else if d <= 4 {
            "3-4"
        } else {
            "5+"
        }
    };
    let mut local_identity = BTreeSet::new();
    for (i, token) in stimulus.tokens.iter().enumerate() {
        if !eligible(i, token) {
            continue;
        }
        let word = normalize(token);
        let role_name = role(i);
        let field = if i < stimulus.title_end {
            "title"
        } else {
            "body"
        };
        let left_distance = SOURCE_POS.abs_diff(i);
        let right_distance = TARGET_POS.abs_diff(i);
        features.add(format!("token:{word}"));
        features.add(format!("token:{word}@role={role_name}"));
        features.add(format!("token:{word}@field={field}"));
        features.add(format!(
            "token:{word}@left-distance={}",
            dist_bucket(left_distance)
        ));
        features.add(format!(
            "token:{word}@right-distance={}",
            dist_bucket(right_distance)
        ));
        if left_distance.min(right_distance) <= 3 {
            features.add(format!("token:{word}@nearby"));
        }
        if i + 1 == SOURCE_POS {
            features.add(format!("immediate:left={word}"));
        }
        if i == TARGET_POS + 1 {
            features.add(format!("immediate:right={word}"));
        }
        if !word.is_empty() && !NEUTRAL_WORDS.iter().any(|w| w.eq_ignore_ascii_case(&word)) {
            local_identity.insert(word.clone());
        }
        if SUPPORT.iter().any(|cue| cue.eq_ignore_ascii_case(&word)) {
            features.set(format!("support@{role_name}"), 1);
        }
        if NEGATIVE.iter().any(|cue| cue.eq_ignore_ascii_case(&word)) {
            features.set(format!("contradiction@{role_name}"), 1);
        }
    }
    for i in 0..stimulus.tokens.len() {
        if !eligible(i, &stimulus.tokens[i]) {
            continue;
        }
        let field = if i < stimulus.title_end {
            "title"
        } else {
            "body"
        };
        for width in [2usize, 3] {
            if i + width > stimulus.tokens.len() {
                continue;
            }
            if (i..i + width)
                .any(|position| (position < stimulus.title_end) != (i < stimulus.title_end))
            {
                continue;
            }
            let span = &stimulus.tokens[i..i + width];
            if span
                .iter()
                .enumerate()
                .any(|(offset, token)| !eligible(i + offset, token))
            {
                continue;
            }
            if span.iter().any(|token| {
                token.eq_ignore_ascii_case(source) || token.eq_ignore_ascii_case(target)
            }) {
                continue;
            }
            let ngram = span
                .iter()
                .map(|t| normalize(t))
                .collect::<Vec<_>>()
                .join("_");
            features.add(format!("ngram{width}:{ngram}@field={field}"));
            if (i..i + width).all(|position| {
                SOURCE_POS
                    .abs_diff(position)
                    .min(TARGET_POS.abs_diff(position))
                    <= 3
            }) {
                features.add(format!("ngram{width}:{ngram}@nearby"));
            }
        }
    }
    features.set(
        "field:source_title",
        u16::from(SOURCE_POS < stimulus.title_end),
    );
    features.set(
        "field:target_title",
        u16::from(TARGET_POS < stimulus.title_end),
    );
    features.set(
        "field:endpoints_same",
        u16::from((SOURCE_POS < stimulus.title_end) == (TARGET_POS < stimulus.title_end)),
    );
    features.set(
        "field:boundary_between",
        u16::from(SOURCE_POS < stimulus.title_end && TARGET_POS >= stimulus.title_end),
    );
    features.set(
        "context:local_token_count_bin",
        local_identity.len().min(5) as u16,
    );
    features.set(
        "context:has_any_local_token",
        u16::from(!local_identity.is_empty()),
    );
    let has_support = features.0.keys().any(|k| k.starts_with("support@"));
    let has_contradiction = features.0.keys().any(|k| k.starts_with("contradiction@"));
    features.set("context:has_support", u16::from(has_support));
    features.set("context:has_contradiction", u16::from(has_contradiction));
    // `r` is used only to assert that the selected observer has a frozen relation.
    debug_assert_eq!(r.candidate_index, stimulus.candidate);
    features
}

pub(super) fn family_route(stimulus: &Stimulus, remove_candidates: bool) -> Option<usize> {
    let (source, target) = candidate_words(stimulus.candidate);
    let low = SOURCE_POS.saturating_sub(8);
    let high = (TARGET_POS + 9).min(stimulus.tokens.len());
    let mut masks = [0u32; 3];
    for (i, token) in stimulus.tokens[low..high].iter().enumerate() {
        let position = low + i;
        if remove_candidates
            && (position == SOURCE_POS
                || position == TARGET_POS
                || token.eq_ignore_ascii_case(source)
                || token.eq_ignore_ascii_case(target))
        {
            continue;
        }
        for family in 0..3 {
            if let Some(marker_index) = marker_names(family)
                .iter()
                .position(|m| m.eq_ignore_ascii_case(token))
            {
                masks[family] |= 1u32 << marker_index;
            }
        }
    }
    let counts = masks.map(u32::count_ones);
    let maximum = counts.into_iter().max()?;
    let mut winners = counts
        .iter()
        .enumerate()
        .filter(|(_, count)| **count == maximum);
    let winner = winners.next()?.0;
    winners.next().is_none().then_some(winner)
}

pub(super) fn add_map_value(out: &mut FeatureMap, key: &str, value: u16) {
    if value > 0 {
        out.set(key.to_string(), value);
    }
}

pub(super) fn pair_features(left: &Stimulus, right: &Stimulus) -> FeatureMap {
    let a = local_features(left);
    let b = local_features(right);
    let (source, target) = candidate_words(left.candidate);
    let local_set = |stimulus: &Stimulus| -> BTreeSet<String> {
        stimulus
            .tokens
            .iter()
            .enumerate()
            .filter_map(|(i, t)| {
                if i == SOURCE_POS
                    || i == TARGET_POS
                    || t.eq_ignore_ascii_case(source)
                    || t.eq_ignore_ascii_case(target)
                    || NEUTRAL_WORDS.iter().any(|w| w.eq_ignore_ascii_case(t))
                {
                    return None;
                }
                let nearest = SOURCE_POS.abs_diff(i).min(TARGET_POS.abs_diff(i));
                (nearest <= 3).then(|| normalize(t))
            })
            .collect()
    };
    let role_set = |stimulus: &Stimulus, wanted: &str| -> BTreeSet<String> {
        stimulus
            .tokens
            .iter()
            .enumerate()
            .filter_map(|(i, t)| {
                if i == SOURCE_POS
                    || i == TARGET_POS
                    || t.eq_ignore_ascii_case(source)
                    || t.eq_ignore_ascii_case(target)
                    || NEUTRAL_WORDS.iter().any(|w| w.eq_ignore_ascii_case(t))
                {
                    return None;
                }
                let current = if i < SOURCE_POS {
                    "before"
                } else if i < TARGET_POS {
                    "between"
                } else {
                    "after"
                };
                (current == wanted && SOURCE_POS.abs_diff(i).min(TARGET_POS.abs_diff(i)) <= 3)
                    .then(|| normalize(t))
            })
            .collect()
    };
    let bin = |n: usize| -> u16 { n.min(3) as u16 };
    let left_set = local_set(left);
    let right_set = local_set(right);
    let mut out = FeatureMap::default();
    out.set(
        "pair:shared-local-count",
        bin(left_set.intersection(&right_set).count()),
    );
    out.set("pair:left-local-count", bin(left_set.len()));
    out.set("pair:right-local-count", bin(right_set.len()));
    out.set(
        "pair:symmetric-difference",
        bin(left_set.symmetric_difference(&right_set).count()),
    );
    let left_ngrams = local_ngrams(left);
    let right_ngrams = local_ngrams(right);
    out.set(
        "pair:shared-local-ngram-count",
        bin(left_ngrams.intersection(&right_ngrams).count()),
    );
    for role in ["before", "between", "after"] {
        let x = role_set(left, role);
        let y = role_set(right, role);
        out.set(
            format!("pair:{role}-shared"),
            bin(x.intersection(&y).count()),
        );
        out.set(format!("pair:{role}-left-count"), bin(x.len()));
        out.set(format!("pair:{role}-right-count"), bin(y.len()));
    }
    for (name, position) in [
        ("left-neighbor", SOURCE_POS - 1),
        ("right-neighbor", TARGET_POS + 1),
    ] {
        let l = normalize(&left.tokens[position]);
        let rr = normalize(&right.tokens[position]);
        out.set(format!("pair:{name}-equal"), u16::from(l == rr));
    }
    out.set(
        "pair:support-equal",
        u16::from(
            a.0.contains_key("context:has_support") == b.0.contains_key("context:has_support"),
        ),
    );
    out.set(
        "pair:contradiction-any",
        u16::from(
            a.0.contains_key("context:has_contradiction")
                || b.0.contains_key("context:has_contradiction"),
        ),
    );
    out.set(
        "pair:left-has-support",
        u16::from(a.0.get("context:has_support") == Some(&1)),
    );
    out.set(
        "pair:right-has-support",
        u16::from(b.0.get("context:has_support") == Some(&1)),
    );
    out.set(
        "pair:left-has-contradiction",
        u16::from(a.0.get("context:has_contradiction") == Some(&1)),
    );
    out.set(
        "pair:right-has-contradiction",
        u16::from(b.0.get("context:has_contradiction") == Some(&1)),
    );
    out.set(
        "pair:one-context-empty",
        u16::from(left_set.is_empty() || right_set.is_empty()),
    );
    out.set(
        "pair:both-context-empty",
        u16::from(left_set.is_empty() && right_set.is_empty()),
    );
    let field_a = (SOURCE_POS < left.title_end, TARGET_POS < left.title_end);
    let field_b = (SOURCE_POS < right.title_end, TARGET_POS < right.title_end);
    out.set("pair:field-layout-equal", u16::from(field_a == field_b));
    // Probe input deliberately excludes candidate identity and both candidate tokens.
    out
}

fn local_ngrams(stimulus: &Stimulus) -> BTreeSet<String> {
    let (source, target) = candidate_words(stimulus.candidate);
    let eligible = |i: usize, token: &str| {
        i != SOURCE_POS
            && i != TARGET_POS
            && !token.eq_ignore_ascii_case(source)
            && !token.eq_ignore_ascii_case(target)
            && !NEUTRAL_WORDS.iter().any(|w| w.eq_ignore_ascii_case(token))
            && SOURCE_POS.abs_diff(i).min(TARGET_POS.abs_diff(i)) <= 3
    };
    let mut out = BTreeSet::new();
    for width in [2usize, 3] {
        for start in 0..stimulus.tokens.len().saturating_sub(width - 1) {
            let end = start + width;
            if end > stimulus.tokens.len()
                || (start..end).any(|i| !eligible(i, &stimulus.tokens[i]))
                || (start..end).any(|i| (i < stimulus.title_end) != (start < stimulus.title_end))
            {
                continue;
            }
            out.insert(
                stimulus.tokens[start..end]
                    .iter()
                    .map(|t| normalize(t))
                    .collect::<Vec<_>>()
                    .join("_"),
            );
        }
    }
    out
}

pub(super) fn label_for_pair(a: Option<usize>, b: Option<usize>) -> usize {
    match (a, b) {
        (Some(x), Some(y)) if x == y => SAME,
        (Some(_), Some(_)) => DIFFERENT,
        _ => PAIR_UNKNOWN,
    }
}

pub(super) fn pair_rows(stimuli: &[Stimulus], candidate: usize, training: bool) -> Vec<Row> {
    let subset = stimuli
        .iter()
        .filter(|s| s.candidate == candidate && (s.group < TRAIN_GROUPS) == training)
        .collect::<Vec<_>>();
    let mut rows = Vec::new();
    for i in 0..subset.len() {
        for j in i + 1..subset.len() {
            if subset[i].group != subset[j].group {
                continue;
            }
            let label = label_for_pair(subset[i].label, subset[j].label);
            rows.push(Row {
                group: subset[i].group,
                label,
                features: pair_features(subset[i], subset[j]),
            });
        }
    }
    rows
}
