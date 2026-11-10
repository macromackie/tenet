use alloc::collections::BTreeMap;
use core::ops::Range;

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Parser, Tag, TagEnd};

use crate::document::DocumentProblem;

pub(crate) struct Markdown {
    pub body: String,
    pub sections: BTreeMap<String, Range<usize>>,
}

/// An H1 or H2 heading outside lists and block quotes, which starts a section.
struct Heading {
    /// Where the section's text starts.
    end: usize,
    level: HeadingLevel,
    name: String,
    /// Where the heading starts.
    start: usize,
}

/// `body` without the example code blocks at the `excluded` byte ranges.
#[expect(
    clippy::string_slice,
    reason = "pulldown-cmark code block spans fall on character boundaries of `body`"
)]
fn strip_examples(body: &str, excluded: Vec<Range<usize>>) -> String {
    let mut authored = String::new();
    let mut cursor = 0;
    for range in excluded {
        authored.push_str(&body[cursor..range.start]);
        cursor = range.end;
    }
    authored.push_str(&body[cursor..]);
    authored
}

/// The section headings of `body` in document order, at document offsets, and the byte ranges of
/// its example code blocks within `body`.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "`offset` plus a pulldown-cmark span stays within the document that `body` ends, \
              which parse caps at 65536 bytes"
)]
fn scan(body: &str, offset: usize) -> (Vec<Heading>, Vec<Range<usize>>) {
    let mut headings = Vec::new();
    let mut open = None;
    let mut nesting = 0_usize;
    let mut excluded = Vec::new();
    for (event, span) in Parser::new(body).into_offset_iter() {
        match event {
            Event::Start(Tag::BlockQuote(_) | Tag::List(_) | Tag::Item) => nesting += 1,
            Event::End(TagEnd::BlockQuote(_) | TagEnd::List(_) | TagEnd::Item) => {
                nesting = nesting.saturating_sub(1);
            },
            Event::Start(Tag::Heading {
                level: level @ (HeadingLevel::H1 | HeadingLevel::H2),
                ..
            }) if nesting == 0 => {
                open = Some(Heading {
                    end: offset + span.end,
                    level,
                    name: String::new(),
                    start: offset + span.start,
                });
            },
            Event::End(TagEnd::Heading(HeadingLevel::H1 | HeadingLevel::H2)) if nesting == 0 => {
                headings.extend(open.take());
            },
            Event::Text(text) | Event::Code(text) => {
                if let Some(heading) = &mut open {
                    heading.name.push_str(&text);
                }
            },
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info)))
                if info.contains("tenet:") =>
            {
                excluded.push(span);
            },
            Event::Start(_)
            | Event::End(_)
            | Event::InlineMath(_)
            | Event::DisplayMath(_)
            | Event::Html(_)
            | Event::InlineHtml(_)
            | Event::FootnoteReference(_)
            | Event::SoftBreak
            | Event::HardBreak
            | Event::Rule
            | Event::TaskListMarker(_) => {},
        }
    }
    (headings, excluded)
}

#[expect(
    clippy::arithmetic_side_effects,
    reason = "`offset` plus the length of `body` is the length of the document that `body` ends"
)]
pub(crate) fn parse(body: &str, offset: usize) -> Result<Markdown, DocumentProblem> {
    let (headings, excluded) = scan(body, offset);
    let ends = headings
        .iter()
        .skip(1)
        .map(|next| next.start)
        .chain([offset + body.len()]);
    let mut sections = BTreeMap::new();
    for (heading, end) in headings.iter().zip(ends) {
        if sections
            .insert(heading.name.clone(), heading.end..end)
            .is_some()
        {
            return Err(DocumentProblem::DuplicateSection {
                name: heading.name.clone(),
            });
        }
    }
    Ok(Markdown {
        body: strip_examples(body, excluded).trim().to_owned(),
        sections,
    })
}

/// Where the last section of `body` starts, at a document offset, when it is an H2 named `name`.
pub(crate) fn find_final_section(body: &str, offset: usize, name: &str) -> Option<usize> {
    scan(body, offset)
        .0
        .pop()
        .filter(|heading| heading.level == HeadingLevel::H2 && heading.name == name)
        .map(|heading| heading.start)
}
