use std::{io::Write, path::Path};

use tenet_checks::behavior::Outcome;

use super::{
    super::audit::write_witness_header,
    ScanOutput,
    collect_witnesses,
    count_synthesized,
    label_mode,
};
use crate::{
    error::CommandError,
    output,
    report_format::{format_dim, format_display, format_marker, quote_argument},
};

pub(super) fn write_scan_terminal(
    out: &mut impl Write,
    payload: &ScanOutput<'_>,
) -> Result<(), CommandError> {
    let synthesized = count_synthesized(payload.reports);
    let catalog_count = payload
        .catalog
        .map_or(0, |catalog| catalog.candidates.len());
    writeln!(
        out,
        "{}  {}\n",
        output::format_styled("Tenet Scan").bold(),
        format_dim(&format!(
            "{} \u{b7} {catalog_count} catalog \u{b7} {synthesized} synthesized",
            label_mode(payload),
        )),
    )?;
    write_catalog_section(out, payload)?;
    write_synthesized_section(out, payload)?;
    write_witness_section(out, payload)?;
    write_unresolved_section(out, payload)?;
    for note in payload.notes {
        writeln!(
            out,
            "  {} {}",
            output::format_styled("= note:").cyan(),
            format_display(note),
        )?;
    }
    if !payload.apply && !payload.ephemeral {
        writeln!(
            out,
            "  {} Run `tenet scan --apply` to save contracts into `.contracts/`, or `tenet scan \
             --ephemeral` to save into `.tenet/scan/.contracts/`.",
            output::format_styled("= help:").cyan(),
        )?;
    }
    Ok(())
}

fn write_catalog_section(
    out: &mut impl Write,
    payload: &ScanOutput<'_>,
) -> Result<(), CommandError> {
    if !payload.installed.is_empty() {
        return write_pass_entries(
            out,
            "Installed catalog contracts",
            payload
                .installed
                .iter()
                .map(|added| (added.name.as_str(), added.path.as_path())),
        );
    }
    let Some(catalog) = payload
        .catalog
        .filter(|catalog| !catalog.candidates.is_empty())
    else {
        return Ok(());
    };
    let scope_arg = quote_argument(&payload.scope.display().to_string());
    writeln!(
        out,
        "{}  {}",
        output::format_styled("Recommended catalog contracts").bold(),
        format_dim(&catalog.release),
    )?;
    for item in &catalog.candidates {
        writeln!(
            out,
            "  tenet/{}  {}  {}\n    {}",
            output::format_styled(&item.contract).bold(),
            format_dim(&item.path.display().to_string()),
            format_dim(&format!("(--scope {scope_arg})")),
            format_display(&format!("{} {}", item.reason, item.message)),
        )?;
    }
    writeln!(out)?;
    Ok(())
}

fn write_pass_entries<'item>(
    out: &mut impl Write,
    heading: &str,
    entries: impl Iterator<Item = (&'item str, &'item Path)>,
) -> Result<(), CommandError> {
    writeln!(out, "{}", output::format_styled(heading).bold())?;
    for (name, path) in entries {
        writeln!(
            out,
            "  {} {name}  {}",
            format_marker("PASS"),
            format_dim(&path.display().to_string()),
        )?;
    }
    writeln!(out)?;
    Ok(())
}

fn write_synthesized_section(
    out: &mut impl Write,
    payload: &ScanOutput<'_>,
) -> Result<(), CommandError> {
    if !payload.saved.is_empty() {
        let heading = if payload.ephemeral {
            "Saved ephemeral contracts"
        } else {
            "Saved module contracts"
        };
        return write_pass_entries(
            out,
            heading,
            payload
                .saved
                .iter()
                .map(|item| (item.name.as_str(), item.path.as_path())),
        );
    }
    if count_synthesized(payload.reports) == 0 {
        return Ok(());
    }
    writeln!(
        out,
        "{}",
        output::format_styled("Synthesized module contracts").bold()
    )?;
    for report in payload.reports {
        for contract in &report.contracts {
            writeln!(
                out,
                "  {}  {}\n    {}",
                output::format_styled(&contract.name).bold(),
                format_dim(&report.name),
                format_display(&contract.message),
            )?;
        }
    }
    writeln!(out)?;
    Ok(())
}

fn write_unresolved_section(
    out: &mut impl Write,
    payload: &ScanOutput<'_>,
) -> Result<(), CommandError> {
    let unresolved: Vec<_> = payload
        .reports
        .iter()
        .flat_map(|report| {
            report
                .findings
                .iter()
                .filter(|finding| finding.outcome == Outcome::Unresolved)
                .map(move |finding| (report.name.as_str(), finding))
        })
        .collect();
    if unresolved.is_empty() {
        return Ok(());
    }
    writeln!(
        out,
        "{}",
        output::format_styled("Unresolved scan checks").bold()
    )?;
    for (module, finding) in unresolved {
        writeln!(
            out,
            "  {} {}/{}  {}\n    {}",
            format_marker("REVIEW"),
            finding.contract,
            finding.check,
            format_dim(module),
            format_display(&finding.message),
        )?;
    }
    writeln!(out)?;
    Ok(())
}

fn write_witness_section(
    out: &mut impl Write,
    payload: &ScanOutput<'_>,
) -> Result<(), CommandError> {
    let witnesses = collect_witnesses(payload.reports, payload.repro_paths);
    if witnesses.is_empty() {
        return Ok(());
    }
    writeln!(
        out,
        "{}",
        output::format_styled("Live failing repros").bold()
    )?;
    for witness in &witnesses {
        let location = witness.subject.unwrap_or(witness.module);
        write_witness_header(
            out,
            witness.contract,
            witness.check,
            location,
            witness.message,
        )?;
        if let Some(repro_path) = witness.repro_path.as_deref() {
            writeln!(
                out,
                "    {}",
                format_dim(&format!("repro file: {repro_path}")),
            )?;
        }
    }
    writeln!(out)?;
    Ok(())
}
