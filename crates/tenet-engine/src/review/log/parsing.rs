//! Versioned review-log decoding preserves original captured file positions.

use std::collections::HashMap;

use serde::Deserialize;

use super::{
    Assessment,
    CapturedFile,
    Conclusion,
    Entry,
    FORMAT,
    FileResult,
    LogError,
    Revision,
    Role,
    ScopedFile,
};
use crate::RequestedScope;

/// Parse one line of the log.
///
/// An assessment must follow `revision`, its revision, and refer only to files it captured. Format
/// 1, which Tenet 0.8 wrote, listed every assessment's files in full; those move into `revision`,
/// so records made with Tenet 0.8 keep settling their inputs, and `moved_files` remembers where
/// each went.
#[expect(
    clippy::too_many_lines,
    reason = "The steps run in one order and share local state; splitting them would add \
              forwarding functions."
)]
pub(in crate::review) fn parse(
    format: u64,
    line: &str,
    revision: Option<&mut Revision>,
    moved_files: &mut HashMap<CapturedFile, usize>,
) -> Result<Entry, LogError> {
    // The type selects the struct to parse. An internally tagged enum would buffer the line first,
    // which loses the arbitrary-precision numbers serde_json is built with here.
    #[derive(Deserialize)]
    struct Kind {
        #[serde(rename = "type")]
        kind: String,
    }

    #[derive(Deserialize)]
    struct Assessment1 {
        carried_from: Option<u64>,
        conclusion: Option<Conclusion>,
        contract: String,
        contract_hash: String,
        files: Vec<ScopedFile>,
        input: String,
        results: Vec<FileResult>,
        rev: u64,
        scope: RequestedScope,
    }

    let Kind { kind } = serde_json::from_str(line)?;
    Ok(match (format, kind.as_str()) {
        (1 | FORMAT, "revision") => {
            moved_files.clear();
            Entry::Revision(serde_json::from_str(line)?)
        },
        (1 | FORMAT, "record") => Entry::Record(serde_json::from_str(line)?),
        (1 | FORMAT, "assessment") => {
            let (mut assessment, files) = if format == 1 {
                let old: Assessment1 = serde_json::from_str(line)?;
                let assessment = Assessment {
                    rev: old.rev,
                    contract: old.contract,
                    contract_hash: old.contract_hash,
                    input: old.input,
                    scope: old.scope,
                    conclusion: old.conclusion,
                    carried_from: old.carried_from,
                    selected: Vec::new(),
                    support: Vec::new(),
                    results: old.results,
                };
                (assessment, old.files)
            } else {
                (serde_json::from_str(line)?, Vec::new())
            };
            let (rev, contract) = (assessment.rev, &assessment.contract);
            let Some(found) = revision.filter(|candidate| candidate.rev == rev) else {
                return Err(LogError::Unordered {
                    contract: contract.clone(),
                    rev,
                });
            };
            if format == 1 {
                (assessment.selected, assessment.support) =
                    add_files(&mut found.files, moved_files, files);
            }
            if !(assessment.selected.iter())
                .chain(&assessment.support)
                .all(|&position| position < found.files.len())
            {
                return Err(LogError::UncapturedFile {
                    contract: contract.clone(),
                    rev,
                });
            }
            Entry::Assessment(assessment)
        },
        (1 | FORMAT, name) => return Err(LogError::UnknownEntry(name.to_owned())),
        _ => return Err(LogError::UnknownFormat(format)),
    })
}

/// Add one assessment's scoped files to a revision's `files`, keeping one copy of each, and return
/// the positions of its selected files and of its supporting files.
///
/// `positions` remembers where each file already is.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "The files vector is nonempty after the push before it."
)]
pub(in crate::review) fn add_files(
    files: &mut Vec<CapturedFile>,
    positions: &mut HashMap<CapturedFile, usize>,
    scoped: Vec<ScopedFile>,
) -> (Vec<usize>, Vec<usize>) {
    let (mut selected, mut support) = (Vec::new(), Vec::new());
    for ScopedFile { role, file } in scoped {
        let position = *positions.entry(file.clone()).or_insert_with(|| {
            files.push(file);
            files.len() - 1
        });
        match role {
            Role::Selected => selected.push(position),
            Role::Support => support.push(position),
        }
    }
    (selected, support)
}
