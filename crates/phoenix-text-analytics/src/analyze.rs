use crate::{
    LensKind, LensSummary, RankedItem, SentenceBand, SentenceSpan, SourceSpan, TextAnalytics,
};
use compact_str::{CompactString, ToCompactString};
use hashbrown::HashMap;
use memchr::memchr2_iter;

const STOP_WORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "been", "but", "by", "for", "from", "had", "has",
    "have", "he", "her", "hers", "him", "his", "i", "if", "in", "into", "is", "it", "its", "me",
    "my", "of", "on", "or", "our", "she", "so", "that", "the", "their", "them", "then", "there",
    "they", "this", "to", "too", "up", "us", "was", "we", "were", "what", "when", "which", "who",
    "with", "would", "you", "your",
];
const DISTANCE_WORDS: &[&str] = &[
    "felt", "feel", "seemed", "seem", "noticed", "notice", "realized", "realize", "saw", "see",
    "heard", "hear", "thought", "think", "wondered", "wonder", "began", "begin", "started",
    "start", "managed", "manage", "tried", "try",
];
const NEGATION_WORDS: &[&str] = &["no", "not", "never", "nothing", "without", "neither", "nor"];
const POETIC_WORDS: &[&str] = &[
    "gossamer", "velvet", "shimmer", "glitter", "hollow", "luminous", "ethereal", "silken",
    "moonlit", "whisper", "echo", "shadow", "crimson", "azure", "obsidian", "golden",
];
const TECHNICAL_WORDS: &[&str] = &[
    "system",
    "protocol",
    "interface",
    "network",
    "sensor",
    "engine",
    "circuit",
    "algorithm",
    "processor",
    "module",
    "signal",
    "device",
    "thermal",
    "quantum",
    "digital",
    "data",
];
const VIOLENT_WORDS: &[&str] = &[
    "blood", "kill", "killed", "gun", "blade", "wound", "strike", "hit", "crush", "bomb", "war",
    "fight", "shot", "burn", "broke", "dead",
];
const PHYSICAL_WORDS: &[&str] = &[
    "hand", "face", "body", "skin", "mouth", "eyes", "feet", "head", "heart", "breath", "shoulder",
    "finger", "voice", "arm", "leg", "chest",
];

#[derive(Clone, Debug)]
struct Token {
    word: CompactString,
    span: SourceSpan,
}

#[derive(Clone, Copy, Debug)]
struct SentenceSample {
    words: u32,
    span: SourceSpan,
}

#[derive(Clone, Copy, Debug, Default)]
struct ProximityAcc {
    last_index: u32,
    repeats: u32,
}

#[derive(Clone, Copy, Debug)]
struct PhraseAcc {
    first_token: u32,
    count: u32,
    width: u8,
}

pub fn analyze(source: &str) -> TextAnalytics {
    if source.trim().is_empty() {
        return TextAnalytics::default();
    }
    let tokens = tokenize(source);
    if tokens.is_empty() {
        return TextAnalytics::default();
    }
    let sentences = sentence_samples(source, &tokens);
    let sentence_lengths = sentences
        .iter()
        .map(|sentence| sentence.words)
        .collect::<Vec<_>>();
    let sentence_count = sentence_lengths.len().max(1) as u32;
    let bands = sentence_bands(&sentence_lengths);
    let (variety_score, flow_score) = flow_scores(&sentence_lengths, &bands);
    let longest_monotony_run = longest_monotony_run(&sentence_lengths);
    let word_count = tokens.len() as u32;
    let echo = echo_lens(&tokens, word_count);
    let phrases = phrase_lens(&tokens);
    let proximity = proximity_lens(&tokens);
    let cadence = cadence_lens(&sentences);
    let negation = word_set_lens(&tokens, LensKind::Negation, NEGATION_WORDS, word_count);
    let ornament = ornament_lens(&tokens, source, sentence_count);
    let distance = word_set_lens(&tokens, LensKind::Distance, DISTANCE_WORDS, word_count);
    let diction = diction_lens(&tokens, word_count);
    let syllables = tokens
        .iter()
        .map(|token| syllable_count(&token.word))
        .sum::<u32>();
    let grade = reading_grade(word_count, sentence_count, syllables);

    TextAnalytics {
        source_hash: source_fingerprint(source),
        word_count,
        character_count: source.chars().count() as u32,
        sentence_count,
        paragraph_count: paragraph_count(source),
        average_sentence_length: word_count as f32 / sentence_count as f32,
        reading_grade: grade.into(),
        reading_seconds: duration_seconds(word_count, 225),
        speaking_seconds: duration_seconds(word_count, 150),
        flow_score,
        variety_score,
        has_monotony: longest_monotony_run >= 5,
        longest_monotony_run,
        sentence_bands: bands,
        sentence_spans: sentences
            .iter()
            .map(|sentence| SentenceSpan {
                band: sentence_band_index(sentence.words) as u8,
                span: sentence.span,
            })
            .collect::<Vec<_>>()
            .into_boxed_slice(),
        lenses: vec![
            echo, phrases, proximity, cadence, negation, ornament, distance, diction,
        ]
        .into_boxed_slice(),
    }
}

pub fn source_fingerprint(source: &str) -> u64 {
    source
        .as_bytes()
        .iter()
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, &byte| {
            (hash ^ byte as u64).wrapping_mul(0x100_0000_01b3)
        })
}

fn tokenize(source: &str) -> Vec<Token> {
    let mut tokens = Vec::with_capacity(source.len() / 6);
    let mut current = String::with_capacity(20);
    let mut current_start = 0_usize;
    let mut current_end = 0_usize;
    let mut chars = source.char_indices().peekable();
    while let Some((offset, ch)) = chars.next() {
        if ch.is_alphanumeric() {
            if current.is_empty() {
                current_start = offset;
            }
            current.extend(ch.to_lowercase());
            current_end = offset + ch.len_utf8();
            continue;
        }
        let internal_joiner = matches!(ch, '\'' | '’' | '-')
            && !current.is_empty()
            && chars.peek().is_some_and(|(_, next)| next.is_alphanumeric());
        if internal_joiner {
            current.push(if ch == '’' { '\'' } else { ch });
            current_end = offset + ch.len_utf8();
            continue;
        }
        if !current.is_empty() {
            tokens.push(Token {
                word: current.as_str().into(),
                span: SourceSpan::new(current_start as u32, current_end as u32),
            });
            current.clear();
        }
    }
    if !current.is_empty() {
        tokens.push(Token {
            word: current.into(),
            span: SourceSpan::new(current_start as u32, current_end as u32),
        });
    }
    tokens
}

fn sentence_samples(source: &str, tokens: &[Token]) -> Vec<SentenceSample> {
    // The punctuation iterator is memchr-powered for ASCII prose. Token counts
    // are then distributed by the same punctuation boundaries without storing
    // byte ranges in the published snapshot.
    let terminators = memchr2_iter(b'.', b'?', source.as_bytes()).count()
        + source
            .as_bytes()
            .iter()
            .filter(|&&byte| byte == b'!')
            .count();
    if terminators == 0 {
        return vec![SentenceSample {
            words: tokens.len() as u32,
            span: SourceSpan::new(
                tokens.first().map_or(0, |token| token.span.start),
                tokens.last().map_or(0, |token| token.span.end),
            ),
        }];
    }
    let mut sentences = Vec::with_capacity(terminators + 1);
    let mut words = 0_u32;
    let mut in_word = false;
    let mut sentence_start = None;
    let mut sentence_end = 0_usize;
    for (offset, ch) in source.char_indices() {
        if ch.is_alphanumeric() {
            if !in_word {
                words += 1;
                in_word = true;
                sentence_start.get_or_insert(offset);
            }
            sentence_end = offset + ch.len_utf8();
        } else {
            in_word = false;
            if matches!(ch, '.' | '?' | '!') && words > 0 {
                sentences.push(SentenceSample {
                    words,
                    span: SourceSpan::new(
                        sentence_start.unwrap_or(offset) as u32,
                        (offset + ch.len_utf8()) as u32,
                    ),
                });
                words = 0;
                sentence_start = None;
                sentence_end = 0;
            }
        }
    }
    if words > 0 {
        sentences.push(SentenceSample {
            words,
            span: SourceSpan::new(sentence_start.unwrap_or(0) as u32, sentence_end as u32),
        });
    }
    if sentences.is_empty() {
        vec![SentenceSample {
            words: tokens.len() as u32,
            span: SourceSpan::new(
                tokens.first().map_or(0, |token| token.span.start),
                tokens.last().map_or(0, |token| token.span.end),
            ),
        }]
    } else {
        sentences
    }
}

fn paragraph_count(source: &str) -> u32 {
    let mut count = 0_u32;
    let mut paragraph_has_text = false;
    for line in source.lines() {
        if line.trim().is_empty() {
            if paragraph_has_text {
                count += 1;
                paragraph_has_text = false;
            }
        } else {
            paragraph_has_text = true;
        }
    }
    count + u32::from(paragraph_has_text)
}

fn sentence_bands(lengths: &[u32]) -> [SentenceBand; 6] {
    let mut counts = [0_u32; 6];
    for &length in lengths {
        counts[sentence_band_index(length)] += 1;
    }
    let labels = [
        "1 word",
        "2-6 words",
        "7-15 words",
        "16-25 words",
        "26-39 words",
        "40+ words",
    ];
    let total = lengths.len().max(1) as u32;
    std::array::from_fn(|index| SentenceBand {
        label: labels[index],
        count: counts[index],
        percent: ((counts[index] * 100 + total / 2) / total).min(100) as u8,
    })
}

fn sentence_band_index(length: u32) -> usize {
    match length {
        0 | 1 => 0,
        2..=6 => 1,
        7..=15 => 2,
        16..=25 => 3,
        26..=39 => 4,
        _ => 5,
    }
}

fn flow_scores(lengths: &[u32], bands: &[SentenceBand; 6]) -> (u8, u8) {
    let total = lengths.len().max(1) as f32;
    let mean = lengths.iter().map(|&v| v as f32).sum::<f32>() / total;
    let variance = lengths
        .iter()
        .map(|&v| {
            let d = v as f32 - mean;
            d * d
        })
        .sum::<f32>()
        / total;
    let deviation_score = (variance.sqrt() / 8.0).min(1.0) * 100.0;
    let entropy = bands
        .iter()
        .filter(|band| band.count > 0)
        .map(|band| {
            let p = band.count as f32 / total;
            -p * p.log2()
        })
        .sum::<f32>();
    let variety = (entropy / (6.0_f32).log2() * 100.0).clamp(0.0, 100.0);
    (
        variety.round() as u8,
        (deviation_score * 0.6 + variety * 0.4)
            .round()
            .clamp(0.0, 100.0) as u8,
    )
}

fn longest_monotony_run(lengths: &[u32]) -> u32 {
    let mut longest = u32::from(!lengths.is_empty());
    let mut current = longest;
    for pair in lengths.windows(2) {
        if pair[0].abs_diff(pair[1]) <= 3 {
            current += 1;
        } else {
            current = 1;
        }
        longest = longest.max(current);
    }
    longest
}

fn echo_lens(tokens: &[Token], word_count: u32) -> LensSummary {
    let mut counts: HashMap<CompactString, u32> = HashMap::with_capacity(tokens.len() / 3);
    for token in tokens {
        if token.word.chars().count() >= 3 && !is_stop_word(&token.word) {
            *counts.entry(token.word.clone()).or_default() += 1;
        }
    }
    let mut items = ranked_words(counts, word_count, 32).into_vec();
    attach_exact_spans(&mut items, tokens);
    let items = items.into_boxed_slice();
    let count = items.iter().map(|item| item.count).sum();
    LensSummary {
        kind: LensKind::Echo,
        count,
        items,
    }
}

fn phrase_lens(tokens: &[Token]) -> LensSummary {
    let mut phrases: HashMap<u64, PhraseAcc> = HashMap::with_capacity(tokens.len() * 2);
    for width in 2..=5_usize {
        for (start, window) in tokens.windows(width).enumerate() {
            if window
                .iter()
                .filter(|token| !is_stop_word(&token.word))
                .count()
                < 2
            {
                continue;
            }
            let hash = phrase_hash(window);
            let entry = phrases.entry(hash).or_insert(PhraseAcc {
                first_token: start as u32,
                count: 0,
                width: width as u8,
            });
            entry.count += 1;
        }
    }
    let mut repeated = phrases
        .into_iter()
        .filter(|(_, entry)| entry.count >= 2)
        .collect::<Vec<_>>();
    repeated.sort_unstable_by_key(|(_, entry)| std::cmp::Reverse(entry.count));
    repeated.truncate(32);
    let items = repeated
        .into_iter()
        .map(|(hash, entry)| {
            let start = entry.first_token as usize;
            let end = start + entry.width as usize;
            let label = tokens[start..end]
                .iter()
                .map(|token| token.word.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            RankedItem {
                label: label.into(),
                count: entry.count,
                detail: format!("{} occurrences", entry.count).into(),
                spans: tokens
                    .windows(entry.width as usize)
                    .filter(|window| phrase_hash(window) == hash)
                    .map(|window| {
                        SourceSpan::new(window[0].span.start, window[window.len() - 1].span.end)
                    })
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            }
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let count = items.iter().map(|item| item.count).sum();
    LensSummary {
        kind: LensKind::Phrases,
        count,
        items,
    }
}

fn phrase_hash(tokens: &[Token]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64 ^ tokens.len() as u64;
    for token in tokens {
        for &byte in token.word.as_bytes() {
            hash = (hash ^ byte as u64).wrapping_mul(0x100_0000_01b3);
        }
        hash = (hash ^ 0xff).wrapping_mul(0x100_0000_01b3);
    }
    hash
}

fn proximity_lens(tokens: &[Token]) -> LensSummary {
    let mut roots: HashMap<CompactString, ProximityAcc> = HashMap::with_capacity(tokens.len() / 3);
    for (index, token) in tokens.iter().enumerate() {
        if token.word.len() < 4 || is_stop_word(&token.word) {
            continue;
        }
        let root = stem(&token.word);
        let acc = roots.entry(root).or_insert(ProximityAcc {
            last_index: index as u32,
            repeats: 0,
        });
        if (index as u32).saturating_sub(acc.last_index) <= 26 {
            acc.repeats += 1;
        }
        acc.last_index = index as u32;
    }
    let counts = roots
        .into_iter()
        .filter_map(|(root, acc)| (acc.repeats > 0).then_some((root, acc.repeats)))
        .collect();
    let mut items = ranked_words(counts, tokens.len() as u32, 32).into_vec();
    let root_slots = items
        .iter()
        .enumerate()
        .map(|(index, item)| (item.label.clone(), index))
        .collect::<HashMap<_, _>>();
    let mut item_spans = vec![Vec::new(); items.len()];
    for token in tokens {
        if let Some(&slot) = root_slots.get(&stem(&token.word)) {
            item_spans[slot].push(token.span);
        }
    }
    for (item, spans) in items.iter_mut().zip(item_spans) {
        item.spans = spans.into_boxed_slice();
    }
    let items = items.into_boxed_slice();
    let count = items.iter().map(|item| item.count).sum();
    LensSummary {
        kind: LensKind::Proximity,
        count,
        items,
    }
}

fn cadence_lens(sentences: &[SentenceSample]) -> LensSummary {
    let lengths = sentences
        .iter()
        .map(|sentence| sentence.words)
        .collect::<Vec<_>>();
    let mut items = Vec::new();
    let mut run_start = 0_usize;
    for index in 1..=lengths.len() {
        let continues = index < lengths.len() && lengths[index - 1].abs_diff(lengths[index]) <= 3;
        if continues {
            continue;
        }
        let run = index - run_start;
        if run >= 5 {
            items.push(RankedItem {
                label: format!("Sentences {}-{}", run_start + 1, index).into(),
                count: run as u32,
                detail: "similar sentence lengths".into(),
                spans: vec![SourceSpan::new(
                    sentences[run_start].span.start,
                    sentences[index - 1].span.end,
                )]
                .into_boxed_slice(),
            });
        }
        run_start = index;
    }
    for (index, pair) in lengths.windows(2).enumerate() {
        let shift = pair[0].abs_diff(pair[1]);
        if shift >= 12 {
            items.push(RankedItem {
                label: format!("Sentences {}-{}", index + 1, index + 2).into(),
                count: shift,
                detail: "abrupt rhythm shift".into(),
                spans: vec![SourceSpan::new(
                    sentences[index].span.start,
                    sentences[index + 1].span.end,
                )]
                .into_boxed_slice(),
            });
        }
    }
    items.sort_unstable_by_key(|item| std::cmp::Reverse(item.count));
    items.truncate(32);
    let count = items.len() as u32;
    LensSummary {
        kind: LensKind::Cadence,
        count,
        items: items.into_boxed_slice(),
    }
}

fn word_set_lens(tokens: &[Token], kind: LensKind, words: &[&str], total: u32) -> LensSummary {
    let mut counts: HashMap<CompactString, u32> = HashMap::new();
    for token in tokens {
        if words.contains(&token.word.as_str()) {
            *counts.entry(token.word.clone()).or_default() += 1;
        }
    }
    let mut values = counts.into_iter().collect::<Vec<_>>();
    values.sort_unstable_by(|(aw, ac), (bw, bc)| bc.cmp(ac).then_with(|| aw.cmp(bw)));
    let mut items = values
        .into_iter()
        .map(|(label, count)| RankedItem {
            label,
            count,
            detail: percent_detail(count, total),
            spans: Box::new([]),
        })
        .collect::<Vec<_>>();
    attach_exact_spans(&mut items, tokens);
    let items = items.into_boxed_slice();
    let count = items.iter().map(|item| item.count).sum();
    LensSummary { kind, count, items }
}

fn ornament_lens(tokens: &[Token], source: &str, sentence_count: u32) -> LensSummary {
    let lowercase = source.to_ascii_lowercase();
    let similes = lowercase.match_indices(" as if ").count() as u32
        + lowercase.match_indices(" as though ").count() as u32;
    let long_words = tokens
        .iter()
        .filter(|token| token.word.chars().count() >= 11)
        .count() as u32;
    let hyphens = source
        .as_bytes()
        .iter()
        .filter(|&&byte| byte == b'-')
        .count() as u32;
    let mut items = vec![
        RankedItem {
            label: "Long-word density".into(),
            count: long_words,
            detail: "11+ letter words".into(),
            spans: tokens
                .iter()
                .filter(|token| token.word.chars().count() >= 11)
                .map(|token| token.span)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        },
        RankedItem {
            label: "Simile frames".into(),
            count: similes,
            detail: "as if / as though".into(),
            spans: [" as if ", " as though "]
                .into_iter()
                .flat_map(|pattern| {
                    lowercase.match_indices(pattern).map(move |(offset, _)| {
                        SourceSpan::new((offset + 1) as u32, (offset + pattern.len() - 1) as u32)
                    })
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        },
        RankedItem {
            label: "Hyphen stacks".into(),
            count: hyphens,
            detail: "compound texture".into(),
            spans: source
                .match_indices('-')
                .map(|(offset, _)| SourceSpan::new(offset as u32, (offset + 1) as u32))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        },
    ];
    items.retain(|item| item.count > 0);
    let count = long_words
        .saturating_add(similes * 2)
        .saturating_add(hyphens)
        .min(sentence_count.saturating_mul(4));
    LensSummary {
        kind: LensKind::Ornament,
        count,
        items: items.into_boxed_slice(),
    }
}

fn diction_lens(tokens: &[Token], total: u32) -> LensSummary {
    let sets = [
        ("Technical", TECHNICAL_WORDS),
        ("Poetic", POETIC_WORDS),
        ("Physical", PHYSICAL_WORDS),
        ("Violent", VIOLENT_WORDS),
    ];
    let mut items = Vec::new();
    for (label, set) in sets {
        let count = tokens
            .iter()
            .filter(|token| set.contains(&token.word.as_str()))
            .count() as u32;
        if count > 0 {
            items.push(RankedItem {
                label: label.into(),
                count,
                detail: percent_detail(count, total),
                spans: tokens
                    .iter()
                    .filter(|token| set.contains(&token.word.as_str()))
                    .map(|token| token.span)
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            });
        }
    }
    let count = items.iter().map(|item| item.count).sum();
    LensSummary {
        kind: LensKind::Diction,
        count,
        items: items.into_boxed_slice(),
    }
}

fn ranked_words(
    counts: HashMap<CompactString, u32>,
    total: u32,
    limit: usize,
) -> Box<[RankedItem]> {
    let mut values = counts
        .into_iter()
        .filter(|(_, count)| *count > 1)
        .collect::<Vec<_>>();
    values.sort_unstable_by(|(aw, ac), (bw, bc)| bc.cmp(ac).then_with(|| aw.cmp(bw)));
    values.truncate(limit);
    values
        .into_iter()
        .map(|(label, count)| RankedItem {
            label,
            count,
            detail: percent_detail(count, total),
            spans: Box::new([]),
        })
        .collect::<Vec<_>>()
        .into_boxed_slice()
}

fn attach_exact_spans(items: &mut [RankedItem], tokens: &[Token]) {
    let slots = items
        .iter()
        .enumerate()
        .map(|(index, item)| (item.label.clone(), index))
        .collect::<HashMap<_, _>>();
    let mut spans = vec![Vec::new(); items.len()];
    for token in tokens {
        if let Some(&slot) = slots.get(&token.word) {
            spans[slot].push(token.span);
        }
    }
    for (item, spans) in items.iter_mut().zip(spans) {
        item.spans = spans.into_boxed_slice();
    }
}

fn percent_detail(count: u32, total: u32) -> CompactString {
    format!(
        "{count} uses, {:.1}% of words",
        count as f32 * 100.0 / total.max(1) as f32
    )
    .to_compact_string()
}

fn stem(word: &str) -> CompactString {
    for suffix in ["ingly", "edly", "ing", "ied", "ed", "es", "s"] {
        if word.len() > suffix.len() + 3 && word.ends_with(suffix) {
            return word[..word.len() - suffix.len()].into();
        }
    }
    word.into()
}

fn is_stop_word(word: &str) -> bool {
    STOP_WORDS.contains(&word)
}

fn syllable_count(word: &str) -> u32 {
    let mut count = 0_u32;
    let mut previous_vowel = false;
    for byte in word.bytes() {
        let vowel = matches!(byte, b'a' | b'e' | b'i' | b'o' | b'u' | b'y');
        if vowel && !previous_vowel {
            count += 1;
        }
        previous_vowel = vowel;
    }
    if word.len() > 3 && word.ends_with('e') && count > 1 {
        count -= 1;
    }
    count.max(1)
}

fn reading_grade(words: u32, sentences: u32, syllables: u32) -> &'static str {
    let grade = 0.39 * (words as f32 / sentences.max(1) as f32)
        + 11.8 * (syllables as f32 / words.max(1) as f32)
        - 15.59;
    match grade {
        value if value <= 5.0 => "1st-5th Grade",
        value if value <= 8.0 => "6th-8th Grade",
        value if value <= 12.0 => "9th-12th Grade",
        value if value <= 16.0 => "College",
        _ => "Graduate",
    }
}

fn duration_seconds(words: u32, words_per_minute: u32) -> u32 {
    (words as u64 * 60).div_ceil(words_per_minute as u64) as u32
}

#[cfg(test)]
mod tests;
