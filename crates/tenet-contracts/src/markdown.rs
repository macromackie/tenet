use std::{
    collections::BTreeMap,
    ops::Range,
    path::{Component, PathBuf},
};

use anyhow::{Result, bail, ensure};
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Parser, Tag, TagEnd};

use crate::{Example, Verdict};

pub(crate) struct Markdown {
    pub sections: BTreeMap<String, Range<usize>>,
    pub examples: Vec<Example>,
}

pub(crate) fn parse(body: &str, offset: usize, document: &str) -> Result<Markdown> {
    let mut sections = BTreeMap::new();
    let mut examples = Vec::new();
    let mut heading = None;
    let mut current: Option<(String, usize)> = None;
    let mut nesting = 0usize;
    let mut example: Option<(String, usize, String)> = None;
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
                ensure!(
                    current.as_ref().is_some_and(|(name, _)| name == "Examples"),
                    "Tenet examples belong in ## Examples"
                );
                example = Some((info.to_string(), offset + span.start, String::new()));
            }
            Event::Text(text) if example.is_some() => {
                if let Some((_, _, source)) = &mut example {
                    source.push_str(&text);
                }
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some((info, start, source)) = example.take() {
                    examples.push(parse_example(
                        &info,
                        source,
                        examples.len() + 1,
                        document[..start].bytes().filter(|b| *b == b'\n').count() + 1,
                    )?);
                }
            }
            _ => {}
        }
    }
    if let Some((name, start)) = current {
        ensure!(!sections.contains_key(&name), "duplicate section: {name}");
        sections.insert(name, start..document.len());
    }
    let mut names = std::collections::BTreeSet::new();
    for example in &examples {
        ensure!(
            names.insert(&example.name),
            "duplicate example name: {}",
            example.name
        );
    }
    Ok(Markdown { sections, examples })
}

fn parse_example(info: &str, source: String, index: usize, line: usize) -> Result<Example> {
    let words: Vec<_> = info.split_whitespace().collect();
    ensure!(
        words.len() >= 4 && words[1] == "tenet:example" && !words[0].contains(':'),
        "expected LANGUAGE tenet:example expect=VERDICT path=PATH"
    );
    let mut fields = BTreeMap::new();
    for word in &words[2..] {
        let (key, value) = word
            .split_once('=')
            .ok_or_else(|| anyhow::anyhow!("invalid example option: {word}"))?;
        ensure!(
            matches!(key, "expect" | "path" | "name"),
            "unknown example option: {key}"
        );
        ensure!(
            !value.is_empty() && fields.insert(key, value).is_none(),
            "empty or duplicate example option: {key}"
        );
    }
    let expected = match fields.get("expect").copied() {
        Some("pass") => Verdict::Pass,
        Some("fail") => Verdict::Fail,
        Some("not_applicable") => Verdict::NotApplicable,
        Some("uncertain") => Verdict::Uncertain,
        _ => bail!("expect must be pass, fail, not_applicable, or uncertain"),
    };
    let path = PathBuf::from(
        fields
            .get("path")
            .ok_or_else(|| anyhow::anyhow!("example requires path"))?,
    );
    ensure!(
        !path.as_os_str().is_empty()
            && path.components().all(|c| matches!(c, Component::Normal(_)))
            && !path.to_string_lossy().contains('\\'),
        "example path must stay within its scope"
    );
    ensure!(!source.trim().is_empty(), "example source is empty");
    Ok(Example {
        name: fields
            .get("name")
            .map_or_else(|| format!("example-{index}"), |s| (*s).to_owned()),
        path,
        expected,
        source,
        line,
    })
}
