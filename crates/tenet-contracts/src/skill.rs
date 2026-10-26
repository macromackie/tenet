//! Installed agent skills: a folder whose `SKILL.md` names it.

use std::path::Path;

use anyhow::Result;
use serde::Deserialize;

use crate::parse::{DocumentError, conflict_marker, frontmatter, is_kebab, key_line, yaml_error};

/// The longest skill name the Agent Skills specification allows.
const SKILL_NAME_CHARACTERS: usize = 64;

/// The keys every installed skill needs. Other keys, such as Claude Code's `paths`, stay allowed.
#[derive(Deserialize)]
struct Metadata {
    name: String,
    description: String,
}

/// Whether `name` is a valid Agent Skills name: lowercase letters, digits, and single hyphens, at
/// most 64 characters.
pub fn is_skill_name(name: &str) -> bool {
    is_kebab(name) && name.len() <= SKILL_NAME_CHARACTERS
}

/// Check an installed `SKILL.md`: parseable frontmatter whose `name` is a valid skill name equal to
/// its folder's name, and a nonempty `description`.
pub fn validate_skill(path: &Path, document: &str) -> Result<()> {
    let error = |line: usize, message: String| DocumentError {
        path: path.to_owned(),
        line,
        message,
    };
    if let Some(line) = conflict_marker(document) {
        return Err(error(line, "unresolved merge conflict marker".into()).into());
    }
    let (yaml, _) = frontmatter(document).map_err(|cause| error(1, cause.to_string()))?;
    let metadata: Metadata =
        serde_yaml::from_str(&document[yaml.clone()]).map_err(|cause| yaml_error(path, &cause))?;
    let folder = path
        .parent()
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy());
    if !is_skill_name(&metadata.name) {
        return Err(error(
            key_line(document, &yaml, "name:"),
            format!(
                "name {} must use lowercase letters, digits, and single hyphens, at most {SKILL_NAME_CHARACTERS} characters",
                metadata.name
            ),
        )
        .into());
    }
    if folder.as_deref() != Some(metadata.name.as_str()) {
        return Err(error(
            key_line(document, &yaml, "name:"),
            format!(
                "name {} must equal the skill's folder name {}",
                metadata.name,
                folder.unwrap_or_default()
            ),
        )
        .into());
    }
    if metadata.description.trim().is_empty() {
        return Err(error(
            key_line(document, &yaml, "description:"),
            "description must not be empty".into(),
        )
        .into());
    }
    Ok(())
}
