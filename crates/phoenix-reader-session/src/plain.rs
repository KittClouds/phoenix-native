use crate::{
    ByteRange, Chapter, DocumentBinding, MappingKind, MappingRun, NarrationPlan, PlanSpec, Result,
    Segment,
};

/// A single plain-text chapter. Nonempty lines are paragraph units. Whitespace
/// at paragraph edges is explicitly omitted, never silently changed by a provider.
/// This does not discover chapters or claim sentence/word alignment.
pub fn plan_plain_chapter(source: &str, document: DocumentBinding) -> Result<NarrationPlan> {
    if source.len() > phoenix_workspace::MAX_DOCUMENT_BYTES {
        return Err(crate::Error::Invalid("plain chapter size"));
    }
    let mut spoken = String::with_capacity(source.len());
    let mut mappings = Vec::new();
    let mut segments = Vec::new();
    let mut source_at = 0;
    for line in source.split_inclusive('\n') {
        let trimmed = line.trim();
        let lead = line.len() - line.trim_start().len();
        let body_start = source_at + lead;
        let body_end = body_start + trimmed.len();
        let end = source_at + line.len();
        if trimmed.is_empty() {
            push(
                &mut mappings,
                source_at,
                end,
                spoken.len(),
                spoken.len(),
                MappingKind::Omit,
            );
        } else {
            push(
                &mut mappings,
                source_at,
                body_start,
                spoken.len(),
                spoken.len(),
                MappingKind::Omit,
            );
            let first = spoken.len();
            spoken.push_str(trimmed);
            push(
                &mut mappings,
                body_start,
                body_end,
                first,
                spoken.len(),
                MappingKind::Copy,
            );
            push(
                &mut mappings,
                body_end,
                end,
                spoken.len(),
                spoken.len(),
                MappingKind::Omit,
            );
            segments.push(Segment {
                chapter: 0,
                sentence: segments.len() as u32,
                source: range(body_start, body_end),
                spoken: range(first, spoken.len()),
            });
        }
        source_at = end;
    }
    NarrationPlan::new(
        source,
        PlanSpec {
            document,
            planner: *blake3::hash(b"phoenix.plain-chapter/trim-paragraph-edges/v1").as_bytes(),
            pronunciation: *blake3::hash(b"phoenix.pronunciation/identity-v1").as_bytes(),
            rules: vec![*blake3::hash(b"unicode-edge-whitespace-omit/v1").as_bytes()].into(),
            spoken: spoken.into_boxed_str(),
            mappings: mappings.into(),
            chapters: vec![Chapter {
                source: range(0, source.len()),
            }]
            .into(),
            segments: segments.into(),
        },
    )
}
fn range(start: usize, end: usize) -> ByteRange {
    ByteRange {
        start: start as u32,
        end: end as u32,
    }
}
fn push(out: &mut Vec<MappingRun>, a: usize, b: usize, c: usize, d: usize, kind: MappingKind) {
    if a != b {
        out.push(MappingRun {
            source: range(a, b),
            spoken: range(c, d),
            kind,
            rule: if kind == MappingKind::Copy { 0 } else { 1 },
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn plan(source: &str) -> NarrationPlan {
        plan_plain_chapter(
            source,
            DocumentBinding {
                workspace: [1; 32],
                entry: 1,
                revision: 1,
                content: *blake3::hash(source.as_bytes()).as_bytes(),
            },
        )
        .unwrap()
    }
    #[test]
    fn preserves_prose_and_unicode_while_accounting_for_all_whitespace() {
        let source =
            "\t\t Indented prose.\r\n\r\n\u{2003}“Drop your weapons!”\r\n\r\nCafé stays.  ";
        let plan = plan(source);
        let spec = plan.spec();
        assert_eq!(spec.segments.len(), 3);
        assert_eq!(
            spec.segments[1].spoken.slice(&spec.spoken).unwrap(),
            "“Drop your weapons!”"
        );
        assert_eq!(
            spec.segments[2].source.slice(source).unwrap(),
            "Café stays."
        );
        for m in &spec.mappings {
            if m.kind == MappingKind::Omit {
                assert!(m
                    .source
                    .slice(source)
                    .unwrap()
                    .chars()
                    .all(char::is_whitespace));
            }
        }
        crate::validate_mapping(source, &spec.spoken, &spec.mappings, spec.rules.len()).unwrap();
    }
    #[test]
    fn preserves_internal_spacing_quotes_and_content_identity() {
        let p = plan(" \"a  b\"\n");
        let q = plan(" \"a b\"\n");
        assert_eq!(&*p.spec().spoken, "\"a  b\"");
        assert_ne!(p.id(), q.id());
    }
}
