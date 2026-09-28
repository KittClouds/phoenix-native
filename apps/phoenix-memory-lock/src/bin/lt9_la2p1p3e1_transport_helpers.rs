pub(super) fn focal_term(context: &str, pair: &[String; 2]) -> Option<String> {
    let mut rest = context;
    while let Some(open) = rest.find('[') {
        let after_open = &rest[open + 1..];
        let close = after_open.find(']')?;
        let token = after_open[..close].trim();
        if token.eq_ignore_ascii_case(&pair[0]) {
            return Some(pair[0].clone());
        }
        if token.eq_ignore_ascii_case(&pair[1]) {
            return Some(pair[1].clone());
        }
        rest = &after_open[close + 1..];
    }
    None
}

pub(super) fn strip_focal(context: &str) -> String {
    context.replace('[', "").replace(']', "")
}

pub(super) fn query_tokens(context: &str) -> Vec<(String, bool)> {
    let chars: Vec<_> = context.char_indices().collect();
    let mut tokens = Vec::new();
    let mut cursor = 0usize;
    while cursor < chars.len() {
        let (start_byte, start_char) = chars[cursor];
        if !start_char.is_alphanumeric() && start_char != '_' {
            cursor += 1;
            continue;
        }
        let start = cursor;
        cursor += 1;
        while cursor < chars.len() && (chars[cursor].1.is_alphanumeric() || chars[cursor].1 == '_')
        {
            cursor += 1;
        }
        let end_byte = if cursor < chars.len() {
            chars[cursor].0
        } else {
            context.len()
        };
        let is_focal = start_byte > 0
            && context[..start_byte].ends_with('[')
            && end_byte < context.len()
            && context[end_byte..].starts_with(']');
        let text: String = chars[start..cursor]
            .iter()
            .map(|(_, ch)| ch.to_lowercase().to_string())
            .collect();
        tokens.push((text, is_focal));
    }
    tokens
}
