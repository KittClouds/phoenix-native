use super::*;
use hashbrown::HashSet;

pub(super) fn tokenize<'a>(text: &'a str) -> Vec<Token<'a>> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(text.len() / 6);
    let mut start = None;
    for (i, byte) in bytes.iter().copied().enumerate() {
        if byte.is_ascii_alphanumeric() {
            if start.is_none() {
                start = Some(i);
            }
        } else if let Some(begin) = start.take() {
            out.push(Token {
                text: &text[begin..i],
                start: begin,
                end: i,
            });
        }
    }
    if let Some(begin) = start {
        out.push(Token {
            text: &text[begin..bytes.len()],
            start: begin,
            end: bytes.len(),
        });
    }
    out
}

pub(super) fn nearest_pair(
    tokens: &[Token<'_>],
    a: &str,
    b: &str,
) -> Option<(usize, usize, usize)> {
    let mut last_a = None;
    let mut last_b = None;
    let mut best = None;
    for (index, token) in tokens.iter().enumerate() {
        if token.text.eq_ignore_ascii_case(a) {
            if let Some(other) = last_b {
                consider_pair(index, other, &mut best);
            }
            last_a = Some(index);
        } else if token.text.eq_ignore_ascii_case(b) {
            if let Some(other) = last_a {
                consider_pair(other, index, &mut best);
            }
            last_b = Some(index);
        }
    }
    best
}

pub(super) fn consider_pair(a: usize, b: usize, best: &mut Option<(usize, usize, usize)>) {
    let distance = a.abs_diff(b);
    if best.is_none_or(|old| distance < old.2) {
        *best = Some((a.min(b), a.max(b), distance));
    }
}

pub(super) fn extract_context(
    field: &str,
    tokens: &[Token<'_>],
    left: usize,
    right: usize,
    distance: usize,
    relation: Relation,
    is_title_field: bool,
) -> (String, String, ContextFeatures) {
    let lo = left.saturating_sub(DISPLAY_WINDOW);
    let hi = (right + DISPLAY_WINDOW + 1).min(tokens.len());
    let excerpt = field[tokens[lo].start..tokens[hi - 1].end].to_string();
    let template = tokens[lo..hi]
        .iter()
        .map(|t| {
            if t.text.eq_ignore_ascii_case(relation.a) {
                "<A>".to_string()
            } else if t.text.eq_ignore_ascii_case(relation.b) {
                "<B>".to_string()
            } else {
                t.text.to_ascii_lowercase()
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    let context_lo = left.saturating_sub(WINDOW);
    let context_hi = (right + WINDOW + 1).min(tokens.len());
    let mut lexical = HashSet::new();
    let mut role_tokens = HashSet::new();
    let mut ordered = Vec::new();
    let mut role_counts = [0u16; 3];
    let mut support = false;
    let mut contradiction = false;
    let mut local_token_count = 0u16;
    for (absolute, token) in tokens[context_lo..context_hi].iter().enumerate() {
        let index = context_lo + absolute;
        let word = token.text.to_ascii_lowercase();
        if word == relation.a || word == relation.b {
            ordered.push(None);
            continue;
        }
        local_token_count = local_token_count.saturating_add(1);
        lexical.insert(word.clone());
        let role = if index < left {
            0
        } else if index <= right {
            1
        } else {
            2
        };
        role_counts[role] = role_counts[role].saturating_add(1);
        role_tokens.insert(format!("r{role}:{word}"));
        if index.abs_diff(left) <= 2 {
            role_tokens.insert(format!("l{}:{word}", index.abs_diff(left)));
        }
        if index.abs_diff(right) <= 2 {
            role_tokens.insert(format!("r{}:{word}", index.abs_diff(right)));
        }
        support |= SUPPORT.iter().any(|cue| word == *cue);
        contradiction |= CONTRADICTION.iter().any(|cue| word == *cue);
        ordered.push(Some(word));
    }
    let mut bigrams = HashSet::new();
    let mut trigrams = HashSet::new();
    for width in [2usize, 3] {
        if ordered.len() < width {
            continue;
        }
        for window in ordered.windows(width) {
            if window.iter().any(Option::is_none) {
                continue;
            }
            let gram = window
                .iter()
                .map(|w| w.as_deref().unwrap_or(""))
                .collect::<Vec<_>>()
                .join(" ");
            if width == 2 {
                bigrams.insert(gram);
            } else {
                trigrams.insert(gram);
            }
        }
    }
    let mut tokens = lexical.into_iter().collect::<Vec<_>>();
    let mut role_tokens = role_tokens.into_iter().collect::<Vec<_>>();
    let mut bigrams = bigrams.into_iter().collect::<Vec<_>>();
    let mut trigrams = trigrams.into_iter().collect::<Vec<_>>();
    tokens.sort();
    role_tokens.sort();
    bigrams.sort();
    trigrams.sort();
    let template_hash = sha256(template.as_bytes());
    let features = ContextFeatures {
        tokens,
        role_tokens,
        bigrams,
        trigrams,
        role_counts,
        local_token_count,
        pair_distance: distance.min(u16::MAX as usize) as u16,
        distance_bin: if distance <= 2 {
            0
        } else if distance <= 5 {
            1
        } else if distance <= 12 {
            2
        } else {
            3
        },
        support_cue: support,
        contradiction_cue: contradiction,
        is_title_field,
    };
    (excerpt, template_hash, features)
}

pub(super) fn token_overlap(a: &[String], b: &[String]) -> f32 {
    let bset: HashSet<&str> = b.iter().map(String::as_str).collect();
    let inter = a.iter().filter(|x| bset.contains(x.as_str())).count();
    let union = a.len() + b.len() - inter;
    if union == 0 {
        0.0
    } else {
        inter as f32 / union as f32
    }
}

pub(super) fn pair_features(a: &ContextFeatures, b: &ContextFeatures) -> PairFeatures {
    let shared_tokens = intersection(&a.tokens, &b.tokens);
    let shared_role_tokens = intersection(&a.role_tokens, &b.role_tokens);
    let shared_bigrams = intersection(&a.bigrams, &b.bigrams);
    let shared_trigrams = intersection(&a.trigrams, &b.trigrams);
    PairFeatures {
        token_jaccard: jaccard(a.tokens.len(), b.tokens.len(), shared_tokens.len()),
        bigram_jaccard: jaccard(a.bigrams.len(), b.bigrams.len(), shared_bigrams.len()),
        trigram_jaccard: jaccard(a.trigrams.len(), b.trigrams.len(), shared_trigrams.len()),
        shared_tokens,
        shared_role_tokens,
        shared_bigrams,
        shared_trigrams,
        role_count_abs_delta: std::array::from_fn(|i| a.role_counts[i].abs_diff(b.role_counts[i])),
        token_count_abs_delta: a.local_token_count.abs_diff(b.local_token_count),
        distance_bin_abs_delta: a.distance_bin.abs_diff(b.distance_bin),
        support_cue_equal: a.support_cue == b.support_cue,
        contradiction_cue_equal: a.contradiction_cue == b.contradiction_cue,
        same_field_kind: a.is_title_field == b.is_title_field,
    }
}

pub(super) fn intersection(a: &[String], b: &[String]) -> Vec<String> {
    let set: HashSet<&str> = b.iter().map(String::as_str).collect();
    a.iter()
        .filter(|x| set.contains(x.as_str()))
        .cloned()
        .collect()
}

pub(super) fn jaccard(a: usize, b: usize, intersection: usize) -> f32 {
    let union = a + b - intersection;
    if union == 0 {
        0.0
    } else {
        intersection as f32 / union as f32
    }
}

pub(super) fn feature_token_leaks(
    a: &ContextFeatures,
    b: &ContextFeatures,
    pair: &PairFeatures,
    relation: Relation,
) -> usize {
    let forbidden = [relation.a, relation.b];
    a.tokens
        .iter()
        .chain(&b.tokens)
        .chain(&a.role_tokens)
        .chain(&b.role_tokens)
        .chain(&a.bigrams)
        .chain(&b.bigrams)
        .chain(&a.trigrams)
        .chain(&b.trigrams)
        .chain(&pair.shared_tokens)
        .chain(&pair.shared_role_tokens)
        .chain(&pair.shared_bigrams)
        .chain(&pair.shared_trigrams)
        .filter(|feature| {
            forbidden.iter().any(|word| {
                feature
                    .split(|c: char| !c.is_ascii_alphanumeric())
                    .any(|part| part.eq_ignore_ascii_case(word))
            })
        })
        .count()
}
