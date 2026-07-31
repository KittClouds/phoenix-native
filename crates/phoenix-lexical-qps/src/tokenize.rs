use compact_str::CompactString;

#[derive(Clone, Debug)]
pub(crate) struct TokenOccurrence {
    pub token: CompactString,
    pub position: u32,
    pub segment: u32,
}

pub(crate) fn tokenize(text: &str) -> Vec<TokenOccurrence> {
    let mut output = Vec::with_capacity(text.len() / 6);
    let mut token = String::with_capacity(24);
    tokenize_into(text, &mut output, &mut token);
    output
}

pub(crate) fn tokenize_into(text: &str, output: &mut Vec<TokenOccurrence>, token: &mut String) {
    output.clear();
    token.clear();
    let mut position = 0_u32;
    let mut segment = 0_u32;

    for character in text.chars() {
        if character.is_alphanumeric() || character == '_' {
            token.extend(character.to_lowercase());
            continue;
        }

        flush(token, output, &mut position, segment);
        if matches!(character, '.' | '!' | '?' | '\n' | '\r') {
            segment = segment.saturating_add(1);
        }
    }
    flush(token, output, &mut position, segment);
}

fn flush(token: &mut String, output: &mut Vec<TokenOccurrence>, position: &mut u32, segment: u32) {
    if token.is_empty() {
        return;
    }
    output.push(TokenOccurrence {
        token: CompactString::from(token.as_str()),
        position: *position,
        segment,
    });
    *position = position.saturating_add(1);
    token.clear();
}

#[cfg(test)]
mod tests {
    use super::tokenize;

    #[test]
    fn preserves_positions_and_sentence_segments_without_bit_packing() {
        let tokens = tokenize("Red dragon. Blue dragon!");
        let observed = tokens
            .iter()
            .map(|token| (token.token.as_str(), token.position, token.segment))
            .collect::<Vec<_>>();
        assert_eq!(
            observed,
            vec![
                ("red", 0, 0),
                ("dragon", 1, 0),
                ("blue", 2, 1),
                ("dragon", 3, 1),
            ]
        );
    }

    #[test]
    fn segment_ids_do_not_overflow_a_twenty_bit_mask() {
        let text = (0..80)
            .map(|index| format!("sentence{index}."))
            .collect::<String>();
        let tokens = tokenize(&text);
        assert_eq!(tokens.last().map(|token| token.segment), Some(79));
    }
}
