use alloc::collections::BTreeMap;
use core::ops::Range;

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Parser, Tag, TagEnd};

use crate::parse::DocumentProblem;

pub(crate) struct Markdown {
    pub body: String,
    pub sections: BTreeMap<String, Range<usize>>,
}

/// Record the open section, which ends at `end`, refusing a second section of the same name.
fn close_section(
    sections: &mut BTreeMap<String, Range<usize>>,
    current: Option<(String, usize)>,
    end: usize,
) -> Result<(), DocumentProblem> {
    let Some((name, start)) = current else {
        return Ok(());
    };
    if sections.insert(name.clone(), start..end).is_some() {
        return Err(DocumentProblem::DuplicateSection { name });
    }
    Ok(())
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

#[expect(
    clippy::arithmetic_side_effects,
    reason = "`offset` plus a pulldown-cmark span or the length of `body` stays within the \
              document that `body` ends, which parse caps at 65536 bytes"
)]
pub(crate) fn parse(body: &str, offset: usize) -> Result<Markdown, DocumentProblem> {
    let mut sections = BTreeMap::new();
    let mut heading = None;
    let mut current: Option<(String, usize)> = None;
    let mut nesting = 0_usize;
    let mut excluded = Vec::new();
    for (event, span) in Parser::new(body).into_offset_iter() {
        match event {
            Event::Start(Tag::BlockQuote(_) | Tag::List(_) | Tag::Item) => nesting += 1,
            Event::End(TagEnd::BlockQuote(_) | TagEnd::List(_) | TagEnd::Item) => {
                nesting = nesting.saturating_sub(1);
            },
            Event::Start(Tag::Heading {
                level: HeadingLevel::H1 | HeadingLevel::H2,
                ..
            }) if nesting == 0 => {
                close_section(&mut sections, current.take(), offset + span.start)?;
                heading = Some(String::new());
            },
            Event::End(TagEnd::Heading(HeadingLevel::H1 | HeadingLevel::H2)) if nesting == 0 => {
                current = heading.take().map(|name| (name, offset + span.end));
            },
            Event::Text(text) | Event::Code(text) => {
                if let Some(name) = &mut heading {
                    name.push_str(&text);
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
    close_section(&mut sections, current, offset + body.len())?;
    Ok(Markdown {
        body: strip_examples(body, excluded).trim().to_owned(),
        sections,
    })
}
