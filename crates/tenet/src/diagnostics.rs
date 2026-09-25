use annotate_snippets::{AnnotationKind, Level, Renderer, Snippet};
use anyhow::Result;
use std::{io::Write, path::Path};
use tenet_contracts::{Contract, DocumentError};
use tenet_engine::CheckResult;

pub(crate) fn finding(
    contract: &Contract,
    result: &CheckResult,
    out: &mut impl Write,
) -> Result<()> {
    let path = contract.path.display().to_string();
    let report = Level::WARNING.primary_title(&contract.message).element(
        Snippet::source(&contract.document).path(&path).annotation(
            AnnotationKind::Context
                .span(contract.rules_span.clone())
                .label("contract requirement"),
        ),
    );
    writeln!(out, "{}", Renderer::plain().render(&[report]))?;
    if let Some(reason) = &result.reason {
        writeln!(out, "  {}: {reason}\n", result.path)?;
    } else {
        writeln!(
            out,
            "  Assessed file: {} (file-level judgment)\n",
            result.path
        )?;
    }
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
