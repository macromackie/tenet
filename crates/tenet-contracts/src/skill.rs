//! Installed agent skills: a folder whose `SKILL.md` names it.

use std::path::Path;

use serde::Deserialize;

use crate::parse::{
    DocumentError,
    DocumentProblem,
    convert_yaml_error,
    find_conflict_marker,
    find_key_line,
    is_kebab,
    split_frontmatter,
};

/// The longest skill name the Agent Skills specification allows.
pub(crate) const SKILL_NAME_CHARACTERS: usize = 64;

/// The keys every installed skill needs. Other keys, such as Claude Code's `paths`, stay allowed.
#[derive(Deserialize)]
struct Metadata {
    description: String,
    name: String,
}

/// Whether `name` is a valid Agent Skills name: lowercase letters, digits, and single hyphens, at
/// most 64 characters.
#[must_use]
#[inline]
pub fn is_skill_name(name: &str) -> bool {
    is_kebab(name) && name.len() <= SKILL_NAME_CHARACTERS
}

/// Check an installed `SKILL.md`: parseable frontmatter whose `name` is a valid skill name equal to
/// its folder's name, and a nonempty `description`.
#[inline]
pub fn validate_skill(path: &Path, document: &str) -> Result<(), DocumentError> {
    let error = |line: usize, problem: DocumentProblem| {
        DocumentError {
            line,
            path: path.to_owned(),
            problem,
        }
    };
    if let Some(line) = find_conflict_marker(document) {
        return Err(error(line, DocumentProblem::ConflictMarker));
    }
    let (yaml, _) = split_frontmatter(document).map_err(|cause| error(1, cause.into()))?;
    let metadata: Metadata =
        serde_yaml::from_str(yaml).map_err(|cause| convert_yaml_error(path, cause))?;
    let folder = path
        .parent()
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy());
    if !is_skill_name(&metadata.name) {
        return Err(error(
            find_key_line(yaml, "name:"),
            DocumentProblem::SkillName {
                name: metadata.name,
            },
        ));
    }
    if folder.as_deref() != Some(metadata.name.as_str()) {
        return Err(error(
            find_key_line(yaml, "name:"),
            DocumentProblem::SkillFolder {
                name: metadata.name,
                folder: folder.unwrap_or_default().into_owned(),
            },
        ));
    }
    if metadata.description.trim().is_empty() {
        return Err(error(
            find_key_line(yaml, "description:"),
            DocumentProblem::NoDescription,
        ));
    }
    Ok(())
}
