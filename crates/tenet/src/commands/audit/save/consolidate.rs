use alloc::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::Context as _;
use tenet_checks::behavior::{AuditContractOutput, PinBundleState, merge_staged_pin_bundles};

use super::{CONTRACTS_DIR, SaveCursor, SavedContract, write_contract_folder};
use crate::error::CommandError;

const FALLBACK_MODULE_SLUG: &str = "module";
const FALLBACK_RULES_BODY: &str = "Preserve module behavioral guarantees.";

/// Strip any trailing `#<shard>` suffix from a discovered module name.
pub(super) fn strip_module_shard(module_name: &str) -> &str {
    module_name
        .split_once('#')
        .map_or(module_name, |(base, _)| base)
}

/// Consolidate two or more verified contracts for one base module into a single contract folder
/// with merged behavior-check scripts and one slim `scripts/.lock.json`.
pub(super) fn save_consolidated_group(
    cursor: &mut SaveCursor<'_>,
    base_module: &str,
    contracts: &[&AuditContractOutput],
) -> Result<SavedContract, CommandError> {
    let slug = allocate_unique_slug(base_module, &cursor.seen_names);
    let document = build_consolidated_document(&slug, base_module, contracts);
    let folder_name = format!("{:03}-{slug}", cursor.next_number);
    let virtual_path = Path::new(CONTRACTS_DIR)
        .join(&folder_name)
        .join("CONTRACT.md");
    let parsed = tenet_contracts::parse(&virtual_path, &document)
        .with_context(|| format!("validate consolidated contract {slug:?}"))?;
    let source_locks: Vec<PathBuf> = contracts
        .iter()
        .map(|contract| cursor.state.join(&contract.lock_path))
        .collect();
    write_contract_folder(cursor, slug, &document, |folder| {
        let state = merge_staged_pin_bundles(&source_locks, folder, &parsed.hash)
            .with_context(|| format!("merge behavior scripts in {:?}", folder.display()))?;
        Ok(state == PinBundleState::Written)
    })
}

fn allocate_unique_slug(base_module: &str, seen_names: &BTreeSet<String>) -> String {
    let base_slug = format_module_slug(base_module);
    if !seen_names.contains(&base_slug) {
        return base_slug;
    }
    let mut suffix = 2_usize;
    loop {
        let candidate = format!("{base_slug}-{suffix}");
        if !seen_names.contains(&candidate) {
            return candidate;
        }
        suffix = suffix.saturating_add(1);
    }
}

fn build_consolidated_document(
    slug: &str,
    base_module: &str,
    contracts: &[&AuditContractOutput],
) -> String {
    let mut paths = BTreeSet::new();
    for contract in contracts {
        for path in &contract.paths {
            let trimmed = path.trim();
            if !trimmed.is_empty() {
                let _inserted = paths.insert(trimmed);
            }
        }
    }
    let message = format!("Behavioral guarantees for {base_module}.");
    let mut lines = vec![
        String::from("---"),
        format!("name: {slug}"),
        format!("message: {}", format_yaml_scalar(&message)),
    ];
    if !paths.is_empty() {
        lines.push(String::from("paths:"));
        for path in paths {
            let escaped = path.replace('\'', "''");
            lines.push(format!("  - '{escaped}'"));
        }
    }
    lines.push(String::from("---"));
    lines.push(String::new());
    lines.push(String::from("## Rules"));
    for contract in contracts {
        let heading = if contract.name.trim().is_empty() {
            base_module
        } else {
            contract.name.trim()
        };
        let body = extract_rules_body(&contract.document, &contract.message);
        lines.push(String::new());
        lines.push(format!("### {heading}"));
        lines.push(String::new());
        lines.push(body);
    }
    let mut document = lines.join("\n");
    document.push('\n');
    document
}

fn normalize_top_headings(body: &str) -> String {
    let mut lines = Vec::new();
    let mut in_fence = false;
    for line in body.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            lines.push(line.to_owned());
            continue;
        }
        if !in_fence
            && let Some(rest) = line.strip_prefix("## ").or_else(|| line.strip_prefix("# "))
        {
            lines.push(format!("#### {rest}"));
        } else {
            lines.push(line.to_owned());
        }
    }
    lines.join("\n")
}

fn extract_rules_body(document: &str, fallback_message: &str) -> String {
    let stripped = strip_frontmatter(document).trim_start();
    let without_heading = match stripped.split_once('\n') {
        Some((first_line, rest)) if first_line.trim().eq_ignore_ascii_case("## rules") => {
            rest.trim_start()
        },
        None if stripped.trim().eq_ignore_ascii_case("## rules") => "",
        _ => stripped,
    };
    let demoted = normalize_top_headings(without_heading).trim().to_owned();
    if !demoted.is_empty() {
        return demoted;
    }
    let fallback = fallback_message.trim();
    if fallback.is_empty() {
        String::from(FALLBACK_RULES_BODY)
    } else {
        fallback.to_owned()
    }
}

fn format_module_slug(base_module: &str) -> String {
    let mut slug = String::new();
    let mut last_dash = false;
    for ch in base_module.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            last_dash = false;
            continue;
        }
        if !slug.is_empty() && !last_dash {
            slug.push('-');
            last_dash = true;
        }
    }
    let trimmed = slug.trim_end_matches('-');
    if trimmed.is_empty() || !tenet_contracts::is_kebab(trimmed) {
        String::from(FALLBACK_MODULE_SLUG)
    } else {
        trimmed.to_owned()
    }
}

fn format_yaml_scalar(text: &str) -> String {
    let starts_special = text.starts_with([
        '-', '?', ':', ',', '[', ']', '{', '}', '#', '&', '*', '!', '|', '>', '\'', '"', '%', '@',
        '`', ' ',
    ]);
    let needs_quotes = text.is_empty()
        || starts_special
        || text.contains(": ")
        || text.contains(" #")
        || text.ends_with([':', ' '])
        || text.contains(['\n', '\r', '\t']);
    if needs_quotes {
        let escaped = text.replace('\'', "''");
        format!("'{escaped}'")
    } else {
        text.to_owned()
    }
}

fn strip_frontmatter(document: &str) -> &str {
    let mut lines = document.split_inclusive('\n');
    let Some(first) = lines.next() else {
        return document;
    };
    if first.trim_end() != "---" {
        return document;
    }
    let mut consumed = first.len();
    for line in lines {
        consumed = consumed.saturating_add(line.len());
        if line.trim_end() == "---" {
            return document.get(consumed..).unwrap_or("");
        }
    }
    document
}
