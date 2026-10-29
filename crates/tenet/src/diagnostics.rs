use std::{
    fs,
    io::{self, Write},
    path::Path,
};

use annotate_snippets::{AnnotationKind, Level, Renderer, Snippet};
use tenet_contracts::Contract;
use tenet_engine::Status;

use crate::error::CommandError;

/// The check results a contract's diagnostics list.
const LISTED_STATUSES: [Status; 3] = [Status::Fail, Status::Error, Status::Incomplete];

pub(crate) fn write_requirement(contract: &Contract, out: &mut impl Write) -> io::Result<()> {
    let path = contract.path.display().to_string();
    let report = Level::ERROR.primary_title(&contract.message).element(
        Snippet::source(&contract.document).path(&path).annotation(
            AnnotationKind::Context
                .span(contract.rules_span.clone())
                .label("contract requirement"),
        ),
    );
    writeln!(out, "{}", Renderer::plain().render(&[report]))?;
    Ok(())
}

#[expect(
    clippy::arithmetic_side_effects,
    reason = "`start` and `n` are offsets into the same source text, so their sum stays within \
              its length."
)]
#[expect(
    clippy::shadow_reuse,
    reason = "Each step rebinds the value it converts under the same name."
)]
#[expect(
    clippy::shadow_unrelated,
    reason = "Each block binds its own result under the name the following lines read."
)]
#[expect(
    clippy::string_slice,
    reason = "Each slice bound is a character boundary: an ASCII hex hash prefix, a str::find \
              offset, or a span the parser returns."
)]
#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "The remaining variants take no action here, and listing each would repeat the enum."
)]
pub(crate) fn write_error(
    root: &Path,
    error: &CommandError,
    out: &mut impl Write,
) -> io::Result<()> {
    let document = match error {
        CommandError::Discovery(error) => error.as_document(),
        CommandError::Adoption(error) => error.as_document(),
        CommandError::Review(error) => error.as_document(),
        _ => None,
    };
    if let Some(error) = document {
        let source = fs::read_to_string(root.join(&error.path)).unwrap_or_default();
        let start: usize = source
            .split_inclusive('\n')
            .take(error.line.saturating_sub(1))
            .map(str::len)
            .sum();
        let end = source[start..]
            .find('\n')
            .map_or(source.len(), |offset| start + offset);
        let path = error.path.display().to_string();
        let report = Level::ERROR
            .primary_title(error.problem.to_string())
            .element(
                Snippet::source(&source)
                    .path(&path)
                    .annotation(AnnotationKind::Primary.span(start..end)),
            );
        writeln!(out, "{}", Renderer::plain().render(&[report]))?;
    } else {
        writeln!(out, "error: {}", tenet_engine::Causes(error))?;
    }
    Ok(())
}

pub(crate) struct Failure {
    pub contract: String,
    pub issues: Vec<tenet_engine::CheckResult>,
    pub reason: Option<String>,
}

impl Failure {
    pub(crate) fn write(&self, contracts: &[Contract], out: &mut impl Write) -> io::Result<()> {
        writeln!(out, "\n── Failed: {} ──\n", self.contract)?;
        if let Some(contract) = contracts
            .iter()
            .find(|candidate| candidate.name == self.contract)
        {
            write_requirement(contract, out)?;
        }
        if let Some(reason) = &self.reason {
            writeln!(out, "  {reason}")?;
        }
        for result in &self.issues {
            if !LISTED_STATUSES.contains(&result.status) {
                continue;
            }
            writeln!(
                out,
                "  {}: {}",
                result.path,
                result
                    .reason
                    .as_deref()
                    .unwrap_or("file evidence supports a violation")
            )?;
        }
        Ok(())
    }
}
