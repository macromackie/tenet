use std::{collections::BTreeMap, ops::Range};

use anyhow::{Result, ensure};
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Parser, Tag, TagEnd};

pub(crate) struct Markdown {
    pub sections: BTreeMap<String, Range<usize>>,
    pub body: String,
}

pub(crate) fn parse(body: &str, offset: usize, document: &str) -> Result<Markdown> {
    let mut sections = BTreeMap::new();
    let mut heading = None;
    let mut current: Option<(String, usize)> = None;
    let mut nesting = 0usize;
    let mut excluded_start = None;
    let mut excluded = Vec::new();
    for (event, span) in Parser::new(body).into_offset_iter() {
        match event {
            Event::Start(Tag::BlockQuote(_) | Tag::List(_) | Tag::Item) => {
                nesting += 1;
            }
            Event::End(TagEnd::BlockQuote(_) | TagEnd::List(_) | TagEnd::Item) => {
                nesting = nesting.saturating_sub(1);
            }
            Event::Start(Tag::Heading {
                level: HeadingLevel::H1 | HeadingLevel::H2,
                ..
            }) if nesting == 0 => {
                if let Some((name, start)) = current.take() {
                    ensure!(!sections.contains_key(&name), "duplicate section: {name}");
                    sections.insert(name, start..offset + span.start);
                }
                heading = Some(String::new());
            }
            Event::End(TagEnd::Heading(HeadingLevel::H1 | HeadingLevel::H2)) if nesting == 0 => {
                if let Some(name) = heading.take() {
                    current = Some((name, offset + span.end));
                }
            }
            Event::Text(text) | Event::Code(text) if heading.is_some() => {
                if let Some(name) = &mut heading {
                    name.push_str(&text);
                }
            }
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info)))
                if info.contains("tenet:") =>
            {
                excluded_start = Some(span.start);
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some(start) = excluded_start.take() {
                    excluded.push(start..span.end);
                }
            }
            _ => {}
        }
    }
    if let Some((name, start)) = current {
        ensure!(!sections.contains_key(&name), "duplicate section: {name}");
        sections.insert(name, start..document.len());
    }
    let mut authored = String::new();
    let mut cursor = 0;
    for range in excluded {
        authored.push_str(&body[cursor..range.start]);
        cursor = range.end;
    }
    authored.push_str(&body[cursor..]);
    Ok(Markdown {
        sections,
        body: authored.trim().to_owned(),
    })
}
