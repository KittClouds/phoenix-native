use super::{error, render::Rendered};
use crate::{ByteRange, MappingKind, Result, Segment};
use unicode_segmentation::UnicodeSegmentation;

pub(super) fn segment(rendered: &Rendered, max_bytes: usize) -> Result<Vec<Segment>> {
    let mut segments = Vec::new();
    let mut sentence = 0u32;
    for block in &rendered.blocks {
        let text = &rendered.spoken[block.start as usize..block.end as usize];
        let mut start = block.start as usize;
        for (at, part) in text.split_sentence_bound_indices() {
            if part.trim().is_empty() {
                continue;
            }
            let end = block.start as usize + at + part.len();
            if end < block.end as usize && (honorific(part) || !legal(rendered, end)) {
                continue;
            }
            split(rendered, start, end, max_bytes, sentence, &mut segments)?;
            sentence = sentence
                .checked_add(1)
                .ok_or_else(|| error("sentence-count-budget", 0))?;
            start = end;
        }
    }
    Ok(segments)
}
fn honorific(text: &str) -> bool {
    let word = text.split_whitespace().last().unwrap_or("");
    if ["Dr.", "Mr.", "Mrs.", "Ms.", "Prof.", "Sr.", "Jr.", "St."]
        .iter()
        .any(|w| word.eq_ignore_ascii_case(w))
    {
        return true;
    }
    let mut chars = word.chars();
    matches!((chars.next(),chars.next(),chars.next()),(Some(c),Some('.'),None) if c.is_uppercase())
}
fn legal(r: &Rendered, at: usize) -> bool {
    let i = r.runs.partition_point(|run| run.spoken.end as usize <= at);
    r.runs
        .get(i)
        .is_none_or(|run| run.kind == MappingKind::Copy || run.spoken.start as usize >= at)
}
fn split(
    r: &Rendered,
    mut start: usize,
    end: usize,
    max: usize,
    sentence: u32,
    out: &mut Vec<Segment>,
) -> Result<()> {
    while start < end {
        let mut cut = end;
        if end - start > max {
            let mut last = 0;
            let mut word = 0;
            let mut nonspace = false;
            for (at, g) in r.spoken[start..end].grapheme_indices(true) {
                let candidate = start + at + g.len();
                if candidate - start > max {
                    break;
                }
                nonspace |= g.chars().any(|c| !c.is_whitespace());
                if legal(r, candidate) {
                    last = candidate;
                    if nonspace
                        && (g.chars().all(char::is_whitespace)
                            || matches!(g, "," | ";" | ":" | "—"))
                    {
                        word = candidate;
                    }
                }
            }
            cut = if word > start { word } else { last };
            if cut <= start {
                return Err(error(
                    "indivisible-span-exceeds-byte-budget",
                    source_offset(r, start),
                ));
            }
        }
        // Unicode sentence segmentation may yield whitespace-only spans. Attach
        // ordinary trailing whitespace to the preceding segment when bounded.
        if r.spoken[start..cut].trim().is_empty() {
            return Err(error("empty-sentence-fragment", source_offset(r, start)));
        }
        let spoken = ByteRange {
            start: start as u32,
            end: cut as u32,
        };
        let mut source = ByteRange {
            start: u32::MAX,
            end: 0,
        };
        crate::mapping::project_spoken(&r.runs, spoken, |range| {
            source.start = source.start.min(range.start);
            source.end = source.end.max(range.end);
        });
        if source.start >= source.end {
            return Err(error("segment-has-no-source", source_offset(r, start)));
        }
        let chapter = r.chapters.partition_point(|c| c.end <= source.start);
        if !r.chapters.get(chapter).is_some_and(|c| c.contains(source)) {
            return Err(error("segment-crosses-chapter", source.start as usize));
        }
        if out.len() >= 262_144 {
            return Err(error("segment-count-budget", source.start as usize));
        }
        out.push(Segment {
            chapter: chapter as u32,
            sentence,
            source,
            spoken,
        });
        start = cut;
    }
    Ok(())
}
fn source_offset(r: &Rendered, at: usize) -> usize {
    let i = r.runs.partition_point(|run| run.spoken.end as usize <= at);
    r.runs.get(i).map_or(0, |run| run.source.start as usize)
}
