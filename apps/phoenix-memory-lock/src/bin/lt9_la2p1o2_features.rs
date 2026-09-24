use super::p1o1;
use hashbrown::HashSet;
use serde::Serialize;

#[derive(Clone, Serialize)]
pub struct ContextFeatures {
    pub tokens: Vec<String>,
    pub role_tokens: Vec<String>,
    pub bigrams: Vec<String>,
    pub trigrams: Vec<String>,
    pub role_counts: [u16; 3],
    pub token_count: u16,
    pub near_count: u16,
    pub support_cue: bool,
    pub contradiction_cue: bool,
    pub field_kind: String,
    pub masked_template: String,
    pub structural_signature: String,
}

#[derive(Serialize)]
pub struct PairFeatures {
    pub shared_tokens: Vec<String>,
    pub shared_role_tokens: Vec<String>,
    pub shared_bigrams: Vec<String>,
    pub shared_trigrams: Vec<String>,
    pub token_jaccard: f64,
    pub bigram_jaccard: f64,
    pub trigram_jaccard: f64,
    pub role_count_abs_delta: [u16; 3],
    pub token_count_abs_delta: u16,
    pub near_count_abs_delta: u16,
    pub support_cue_equal: bool,
    pub contradiction_cue_equal: bool,
    pub same_field_kind: bool,
}

#[derive(Clone, Copy)]
struct Span {
    start: usize,
    end: usize,
    focal: bool,
}

pub fn context_features(
    occurrence: &p1o1::Occurrence,
    lemma_a: &str,
    lemma_b: &str,
) -> ContextFeatures {
    let text = occurrence.excerpt.as_bytes();
    let mut spans = Vec::new();
    let mut cursor = 0;
    while cursor < text.len() {
        while cursor < text.len() && !text[cursor].is_ascii_alphanumeric() {
            cursor += 1;
        }
        let start = cursor;
        while cursor < text.len() && text[cursor].is_ascii_alphanumeric() {
            cursor += 1;
        }
        if cursor > start {
            let focal =
                start > 0 && text[start - 1] == b'[' && cursor < text.len() && text[cursor] == b']';
            spans.push(Span {
                start,
                end: cursor,
                focal,
            });
        }
    }
    let focal_index = spans
        .iter()
        .position(|s| s.focal)
        .unwrap_or(spans.len() / 2);
    let mut token_set = HashSet::new();
    let mut role_set = HashSet::new();
    let mut ordered = Vec::<Option<String>>::with_capacity(spans.len());
    let mut role_counts = [0u16; 3];
    let mut structural_bins = [0u8; 6];
    let mut near_count = 0u16;
    let mut support = false;
    let mut contradiction = false;
    let mut template_parts = Vec::with_capacity(spans.len());
    for (index, span) in spans.iter().enumerate() {
        let token = String::from_utf8_lossy(&text[span.start..span.end]).to_ascii_lowercase();
        let is_candidate =
            token.eq_ignore_ascii_case(lemma_a) || token.eq_ignore_ascii_case(lemma_b);
        if span.focal {
            template_parts.push("<FOCAL>".to_owned());
            ordered.push(None);
            continue;
        }
        template_parts.push(token.clone());
        if is_candidate {
            ordered.push(None);
            continue;
        }
        let role = if index < focal_index { 0 } else { 2 };
        role_counts[role] = role_counts[role].saturating_add(1);
        token_set.insert(token.clone());
        let side = if role == 0 { "before" } else { "after" };
        role_set.insert(format!("{side}:{token}"));
        let distance = index.abs_diff(focal_index);
        let structural_bucket = if distance <= 2 {
            if role == 0 {
                0
            } else {
                3
            }
        } else if distance <= 6 {
            if role == 0 {
                1
            } else {
                4
            }
        } else if role == 0 {
            2
        } else {
            5
        };
        structural_bins[structural_bucket] =
            structural_bins[structural_bucket].saturating_add(1).min(3);
        if distance <= 2 {
            near_count = near_count.saturating_add(1);
            role_set.insert(format!("near-{side}-{distance}:{token}"));
        }
        support |= [
            "also",
            "as",
            "equivalent",
            "like",
            "mean",
            "means",
            "same",
            "similar",
            "use",
            "used",
        ]
        .contains(&token.as_str());
        contradiction |= [
            "although",
            "but",
            "different",
            "however",
            "instead",
            "not",
            "rather",
            "unlike",
            "whereas",
        ]
        .contains(&token.as_str());
        ordered.push(Some(token));
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
                .filter_map(|x| x.as_ref())
                .cloned()
                .collect::<Vec<_>>()
                .join(" ");
            if width == 2 {
                bigrams.insert(gram);
            } else {
                trigrams.insert(gram);
            }
        }
    }
    let mut tokens = token_set.into_iter().collect::<Vec<_>>();
    let mut role_tokens = role_set.into_iter().collect::<Vec<_>>();
    let mut bigrams = bigrams.into_iter().collect::<Vec<_>>();
    let mut trigrams = trigrams.into_iter().collect::<Vec<_>>();
    tokens.sort();
    role_tokens.sort();
    bigrams.sort();
    trigrams.sort();
    let masked_template = template_parts.join(" ");
    let structural_signature = format!(
        "{}:{}:{}:{}:{}:{}:{}",
        occurrence.field,
        structural_bins[0],
        structural_bins[1],
        structural_bins[2],
        structural_bins[3],
        structural_bins[4],
        structural_bins[5]
    );
    ContextFeatures {
        token_count: role_counts[0].saturating_add(role_counts[2]),
        tokens,
        role_tokens,
        bigrams,
        trigrams,
        role_counts,
        near_count,
        support_cue: support,
        contradiction_cue: contradiction,
        field_kind: occurrence.field.clone(),
        masked_template,
        structural_signature,
    }
}

pub fn pair_features(a: &ContextFeatures, b: &ContextFeatures) -> PairFeatures {
    PairFeatures {
        shared_tokens: intersection(&a.tokens, &b.tokens),
        shared_role_tokens: intersection(&a.role_tokens, &b.role_tokens),
        shared_bigrams: intersection(&a.bigrams, &b.bigrams),
        shared_trigrams: intersection(&a.trigrams, &b.trigrams),
        token_jaccard: jaccard(&a.tokens, &b.tokens),
        bigram_jaccard: jaccard(&a.bigrams, &b.bigrams),
        trigram_jaccard: jaccard(&a.trigrams, &b.trigrams),
        role_count_abs_delta: std::array::from_fn(|i| a.role_counts[i].abs_diff(b.role_counts[i])),
        token_count_abs_delta: a.token_count.abs_diff(b.token_count),
        near_count_abs_delta: a.near_count.abs_diff(b.near_count),
        support_cue_equal: a.support_cue == b.support_cue,
        contradiction_cue_equal: a.contradiction_cue == b.contradiction_cue,
        same_field_kind: a.field_kind == b.field_kind,
    }
}

fn intersection(a: &[String], b: &[String]) -> Vec<String> {
    let right: HashSet<&str> = b.iter().map(String::as_str).collect();
    a.iter()
        .filter(|x| right.contains(x.as_str()))
        .cloned()
        .collect()
}

pub fn jaccard(a: &[String], b: &[String]) -> f64 {
    let left: HashSet<&str> = a.iter().map(String::as_str).collect();
    let right: HashSet<&str> = b.iter().map(String::as_str).collect();
    let union = left.union(&right).count();
    if union == 0 {
        1.0
    } else {
        left.intersection(&right).count() as f64 / union as f64
    }
}
