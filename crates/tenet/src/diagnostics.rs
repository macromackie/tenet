use annotate_snippets::{AnnotationKind, Level, Renderer, Snippet};
use anyhow::Result;
use std::{io::Write, path::Path};
use tenet_contracts::{Contract, DocumentError};

pub(crate) fn requirement(contract: &Contract, out: &mut impl Write) -> Result<()> {
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

pub(crate) fn error(root: &Path, error: &anyhow::Error, out: &mut impl Write) -> Result<()> {
    if let Some(error) = error.downcast_ref::<DocumentError>() {
        let source = std::fs::read_to_string(root.join(&error.path)).unwrap_or_default();
        let start: usize = source
            .split_inclusive('\n')
            .take(error.line.saturating_sub(1))
            .map(str::len)
            .sum();
        let end = source[start..]
            .find('\n')
            .map_or(source.len(), |n| start + n);
        let path = error.path.display().to_string();
        let report = Level::ERROR.primary_title(&error.message).element(
            Snippet::source(&source)
                .path(&path)
                .annotation(AnnotationKind::Primary.span(start..end)),
        );
        writeln!(out, "{}", Renderer::plain().render(&[report]))?;
    } else {
        writeln!(out, "error: {error:#}")?;
    }
    Ok(())
}

pub(crate) struct Failure {
    pub contract: String,
    pub reason: Option<String>,
    pub issues: Vec<tenet_engine::CheckResult>,
    pub evaluation: bool,
}

impl Failure {
    pub(crate) fn write(&self, contracts: &[Contract], out: &mut impl Write) -> Result<()> {
        use crate::report_format::status_name;
        use tenet_engine::Status;

        writeln!(out, "\n── Failed: {} ──\n", self.contract)?;
        if let Some(contract) = contracts.iter().find(|c| c.name == self.contract) {
            requirement(contract, out)?;
        }
        if let Some(reason) = &self.reason {
            writeln!(out, "  {reason}")?;
        }
        for result in &self.issues {
            if !matches!(
                result.status,
                Status::Fail | Status::Error | Status::Incomplete
            ) && !self.evaluation
            {
                continue;
            }
            let label = result.example.as_deref().unwrap_or(&result.path);
            let fallback = if self.evaluation {
                "example did not match its expectation"
            } else {
                "file evidence supports a violation"
            };
            writeln!(
                out,
                "  {label}: {}",
                result.reason.as_deref().unwrap_or(fallback)
            )?;
            if let Some(expected) = result.expected {
                writeln!(
                    out,
                    "    expected {} · received {}",
                    status_name(Status::from(expected)),
                    status_name(result.status)
                )?;
            }
        }
        Ok(())
    }
}
