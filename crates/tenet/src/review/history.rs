use std::collections::{HashMap, HashSet};

use serde_json::{Value, json};
use similar::{ChangeTag, TextDiff};
use tenet_engine::{
    Review,
    review::{Assessment, CapturedFile, CurrentContract, MAX_BLOB_BYTES},
};

use super::History;
use crate::error::CommandError;

impl<'review> History<'review> {
    pub(super) fn new(
        review: &'review Review,
        entry: &CurrentContract<'_>,
    ) -> Result<Self, CommandError> {
        let name = &entry.contract.name;
        let records = (review.list_records().rev())
            .filter(|record| &record.contract == name)
            .collect();
        let mut inputs = HashSet::from([entry.input.as_str()]);
        let earlier: Vec<_> = (review.list_assessments().rev())
            .filter(|assessment| {
                &assessment.contract == name && inputs.insert(assessment.input.as_str())
            })
            .collect();
        let changes_since = earlier
            .first()
            .map(|old| compute_interdiff(review, old, entry))
            .transpose()?
            .unwrap_or_default();
        Ok(Self {
            records,
            earlier,
            changes_since,
        })
    }
}

/// What changed in the contract's inputs between an earlier assessment and now.
fn compute_interdiff(
    review: &Review,
    old: &Assessment,
    entry: &CurrentContract<'_>,
) -> Result<Vec<Value>, CommandError> {
    let mut changes = Vec::new();
    let contract_path = entry.contract.path.display().to_string();
    let forgotten = format!(
        "contents from r{} are not in the review state, which keeps files up to {} MiB",
        old.rev,
        MAX_BLOB_BYTES >> 20_i32
    );
    if old.contract_hash != entry.contract.hash {
        match review.read_blob(&old.contract_hash)? {
            Some(read_before) => {
                let text = diff(
                    &contract_path,
                    old.rev,
                    &read_before,
                    &entry.contract.document,
                );
                changes.push(json!({"path": contract_path, "diff": text}));
            },
            None => changes.push(json!({"path": contract_path, "note": forgotten})),
        }
    }
    let revision = review.find_revision(old.rev)?;
    let previous_files: HashMap<_, _> = revision
        .list_files(old)
        .map(|(_, file)| (&file.path, file))
        .collect();
    for scoped in &entry.files {
        if let Some(change) = compute_file_interdiff(
            review,
            old,
            entry,
            &scoped.file,
            previous_files.get(&scoped.file.path).copied(),
            &forgotten,
        )? {
            changes.push(change);
        }
    }
    let current_paths: HashSet<_> = entry.files.iter().map(|scoped| &scoped.file.path).collect();
    for (_, file) in revision.list_files(old) {
        if !current_paths.contains(&file.path) {
            let path = file.path.display().to_string();
            changes.push(json!({"path": path, "note": "left the contract's inputs"}));
        }
    }
    Ok(changes)
}

fn compute_file_interdiff(
    review: &Review,
    old: &Assessment,
    entry: &CurrentContract<'_>,
    file: &CapturedFile,
    previous: Option<&CapturedFile>,
    forgotten: &str,
) -> Result<Option<Value>, CommandError> {
    let path = file.path.display().to_string();
    let Some(previous_file) = previous else {
        return Ok(Some(
            json!({"path": path, "note": "entered the contract's inputs"}),
        ));
    };
    if previous_file.after == file.after {
        return Ok((previous_file.before != file.before)
            .then(|| json!({"path": path, "note": "base contents changed"})));
    }
    let before = match previous_file.after.as_deref() {
        Some(hash) => {
            let Some(contents) = review.read_blob(hash)? else {
                return Ok(Some(json!({"path": path, "note": forgotten})));
            };
            contents
        },
        None => String::new(),
    };
    let after = entry
        .changes
        .iter()
        .find(|change| change.path == file.path)
        .and_then(|change| change.after.clone())
        .unwrap_or_default();
    let lines = TextDiff::from_lines(&before, &after);
    let count = |tag| {
        lines
            .iter_all_changes()
            .filter(|change| change.tag() == tag)
            .count()
    };
    let (added, removed) = (count(ChangeTag::Insert), count(ChangeTag::Delete));
    let text = diff(&path, old.rev, &before, &after);
    Ok(Some(
        json!({"path": path, "diff": text, "added": added, "removed": removed}),
    ))
}

fn diff(path: &str, rev: u64, before: &str, after: &str) -> String {
    TextDiff::from_lines(before, after)
        .unified_diff()
        .context_radius(3)
        .header(&format!("{path} (r{rev})"), &format!("{path} (now)"))
        .to_string()
}
