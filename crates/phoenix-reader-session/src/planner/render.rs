use super::error;
use crate::{ByteRange, Digest, MappingKind, MappingRun, Result};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use std::ops::Range;

const MARKUP: u32 = 1;
const DECODE: u32 = 2;
const WHITESPACE: u32 = 3;
const BLOCK: u32 = 4;
const CODE: u32 = 5;
const IMAGE: u32 = 6;
const METADATA: u32 = 7;
pub(super) fn rule_hashes() -> Box<[Digest]> {
    [
        "markdown-syntax-omit/v1",
        "markdown-visible-decode/v1",
        "whitespace-normalize/v1",
        "block-separator/v1",
        "code-block-omit/v1",
        "image-omit/v1",
        "metadata-omit/v1",
    ]
    .map(|r| *blake3::hash(r.as_bytes()).as_bytes())
    .into()
}
pub(super) struct Rendered {
    pub spoken: String,
    pub runs: Vec<MappingRun>,
    pub blocks: Vec<ByteRange>,
    pub chapters: Vec<ByteRange>,
    pub code_blocks: u32,
    pub images: u32,
    pub metadata: u32,
}
struct Builder<'a> {
    source: &'a str,
    out: Rendered,
    cursor: usize,
    block_start: usize,
    pending_break: bool,
}
impl<'a> Builder<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            cursor: 0,
            block_start: 0,
            pending_break: false,
            out: Rendered {
                spoken: String::with_capacity(source.len()),
                runs: Vec::with_capacity((source.len() / 16).min(65_536)),
                blocks: Vec::new(),
                chapters: Vec::new(),
                code_blocks: 0,
                images: 0,
                metadata: 0,
            },
        }
    }
    fn run(&mut self, range: Range<usize>, text: &str, kind: MappingKind, rule: u32) -> Result<()> {
        if range.start != self.cursor || self.source.get(range.clone()).is_none() {
            return Err(error("nonmonotonic-parser-range", range.start));
        }
        if self.out.runs.len() >= 1_000_000 || self.out.spoken.len() + text.len() > 64 * 1024 * 1024
        {
            return Err(error("planner-output-budget", range.start));
        }
        if range.is_empty() && text.is_empty() {
            return Ok(());
        }
        let start = self.out.spoken.len() as u32;
        self.out.spoken.push_str(text);
        let run = MappingRun {
            source: ByteRange {
                start: range.start as u32,
                end: range.end as u32,
            },
            spoken: ByteRange {
                start,
                end: self.out.spoken.len() as u32,
            },
            kind,
            rule,
        };
        // Coalesce only lossless copies or omissions, never distinct replacements.
        if let Some(last) = self.out.runs.last_mut().filter(|last| {
            last.kind == kind
                && last.rule == rule
                && matches!(kind, MappingKind::Copy | MappingKind::Omit)
                && last.source.end == run.source.start
                && last.spoken.end == run.spoken.start
        }) {
            last.source.end = run.source.end;
            last.spoken.end = run.spoken.end;
        } else {
            self.out.runs.push(run);
        }
        self.cursor = range.end;
        Ok(())
    }
    fn omit_to(&mut self, end: usize) -> Result<()> {
        self.run(self.cursor..end, "", MappingKind::Omit, MARKUP)
    }
    fn omit(&mut self, range: Range<usize>, rule: u32) -> Result<()> {
        self.boundary()?;
        self.omit_to(range.start)?;
        self.run(range, "", MappingKind::Omit, rule)
    }
    fn boundary(&mut self) -> Result<()> {
        let end = self.out.spoken.len();
        if end > self.block_start {
            if self.out.spoken[self.block_start..].trim().is_empty() {
                return Err(error("empty-spoken-block", self.cursor));
            }
            self.out.blocks.push(ByteRange {
                start: self.block_start as u32,
                end: end as u32,
            });
            self.block_start = end;
        }
        self.pending_break = !self.out.spoken.is_empty();
        Ok(())
    }
    fn leaf(&mut self, range: Range<usize>, text: &str) -> Result<()> {
        self.omit_to(range.start)?;
        if self.pending_break && !text.is_empty() {
            self.run(self.cursor..self.cursor, "\n", MappingKind::Insert, BLOCK)?;
            self.pending_break = false;
        }
        let raw = &self.source[range.clone()];
        if raw != text {
            // Decoded entities/escapes and normalized code spans are indivisible
            // substitutions; segmenting cannot invent their internal source map.
            let mut normalized = String::with_capacity(text.len());
            let mut previous_space = false;
            for c in text.chars() {
                if c.is_whitespace() {
                    if !previous_space {
                        normalized.push(' ');
                    }
                    previous_space = true;
                } else {
                    normalized.push(c);
                    previous_space = false;
                }
            }
            return self.run(range, &normalized, MappingKind::Replace, DECODE);
        }
        let mut start = 0;
        let mut whitespace = None;
        for (at, c) in text.char_indices() {
            if c.is_whitespace() {
                if whitespace.is_none() {
                    if at > start {
                        self.run(
                            range.start + start..range.start + at,
                            &text[start..at],
                            MappingKind::Copy,
                            0,
                        )?;
                    }
                    whitespace = Some(at);
                }
            } else if let Some(space) = whitespace.take() {
                let original = &text[space..at];
                self.run(
                    range.start + space..range.start + at,
                    " ",
                    if original == " " {
                        MappingKind::Copy
                    } else {
                        MappingKind::Replace
                    },
                    if original == " " { 0 } else { WHITESPACE },
                )?;
                start = at;
            }
        }
        if let Some(space) = whitespace {
            let original = &text[space..];
            self.run(
                range.start + space..range.end,
                " ",
                if original == " " {
                    MappingKind::Copy
                } else {
                    MappingKind::Replace
                },
                if original == " " { 0 } else { WHITESPACE },
            )?;
        } else if start < text.len() {
            self.run(
                range.start + start..range.end,
                &text[start..],
                MappingKind::Copy,
                0,
            )?;
        }
        Ok(())
    }
}

pub(super) fn markdown(source: &str, chapter_level: u8) -> Result<Rendered> {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
        | Options::ENABLE_PLUSES_DELIMITED_METADATA_BLOCKS;
    let mut b = Builder::new(source);
    let mut starts = vec![0usize];
    let mut suppressed = 0usize;
    for (event, range) in Parser::new_ext(source, options).into_offset_iter() {
        if suppressed > 0 {
            match event {
                Event::Start(_) => suppressed += 1,
                Event::End(_) => suppressed -= 1,
                _ => {}
            }
            continue;
        }
        match event {
            Event::Start(Tag::Table(_) | Tag::FootnoteDefinition(_))
            | Event::FootnoteReference(_) => {
                return Err(error("unsupported-table-or-footnote", range.start))
            }
            Event::Start(Tag::HtmlBlock) | Event::Html(_) | Event::InlineHtml(_) => {
                return Err(error("unsupported-raw-html", range.start))
            }
            Event::Start(Tag::CodeBlock(_)) => {
                b.out.code_blocks += 1;
                b.omit(range, CODE)?;
                suppressed = 1;
            }
            Event::Start(Tag::Image { .. }) => {
                b.out.images += 1;
                b.omit(range, IMAGE)?;
                suppressed = 1;
            }
            Event::Start(Tag::MetadataBlock(_)) => {
                b.out.metadata += 1;
                b.omit(range, METADATA)?;
                suppressed = 1;
            }
            Event::Start(Tag::Heading { level, .. }) => {
                b.boundary()?;
                if level as u8 <= chapter_level && starts.last() != Some(&range.start) {
                    starts.push(range.start);
                }
            }
            Event::Start(Tag::Paragraph | Tag::Item) => b.boundary()?,
            Event::End(TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::Item) => b.boundary()?,
            Event::Text(text) | Event::Code(text) => b.leaf(range, &text)?,
            Event::SoftBreak => b.leaf(range, " ")?,
            Event::HardBreak => {
                b.leaf(range, " ")?;
                b.boundary()?;
            }
            Event::Rule => {
                b.boundary()?;
                b.omit_to(range.end)?;
            }
            Event::TaskListMarker(_) => b.omit_to(range.end)?,
            Event::InlineMath(_) | Event::DisplayMath(_) => {
                return Err(error("unsupported-math", range.start))
            }
            Event::Start(_) | Event::End(_) => {}
        }
    }
    b.boundary()?;
    b.omit_to(source.len())?;
    if b.out.spoken.trim().is_empty() {
        return Err(error("no-narratable-text", 0));
    }
    starts.push(source.len());
    b.out.chapters = starts
        .windows(2)
        .filter(|p| p[0] < p[1])
        .map(|p| ByteRange {
            start: p[0] as u32,
            end: p[1] as u32,
        })
        .collect();
    Ok(b.out)
}
