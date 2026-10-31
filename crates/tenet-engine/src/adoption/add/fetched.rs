//! Resolve library requests and prepare their files for a locked installation.

use alloc::collections::BTreeMap;
use std::path::{Path, PathBuf};

use tenet_contracts::{Lock, hash_file, is_kebab, with_name};

use super::{
    AdoptionError,
    Fetched,
    File,
    Kind,
    Origin,
    Rule,
    Unit,
    Upstream,
    decode_text,
    find_obstruction,
    parse_spec,
    read,
};

/// Register `--source ALIAS=URL` values, then fetch `SOURCE/NAME[@REF]`.
pub(super) fn read_request(
    lock: &mut Lock,
    kind: Kind,
    spec: &str,
    sources: &[String],
) -> Result<Fetched, AdoptionError> {
    let (source, name, reference) = parse_spec(spec)?;
    for registration in sources {
        let (alias, url) = registration
            .split_once('=')
            .ok_or(AdoptionError::SourceFormat)?;
        if !is_kebab(alias) {
            return Err(AdoptionError::SourceAlias);
        }
        if url.is_empty() || url.starts_with('-') {
            return Err(AdoptionError::SourceUrl(alias.to_owned()));
        }
        // `tenet` always names the public library, even before a lock exists.
        if let Some(existing) = lock.sources.get(alias)
            && existing != url
        {
            return Err(AdoptionError::SourceTaken {
                alias: alias.to_owned(),
                existing: existing.clone(),
                url: url.to_owned(),
                name: name.to_owned(),
            });
        }
        drop(lock.sources.insert(alias.into(), url.into()));
    }
    let url = lock
        .sources
        .get(source)
        .ok_or_else(|| AdoptionError::UnknownSource(source.to_owned()))?;
    let upstream = Upstream::download(url)?;
    let revision = upstream.resolve(reference)?;
    let files = upstream
        .read_folder(&revision.commit, kind, name)?
        .ok_or_else(|| {
            AdoptionError::NotInLibrary {
                library: [source, name].join("/"),
                reference: revision.reference.clone(),
            }
        })?;
    Ok(Fetched {
        files,
        name: name.into(),
        revision,
        source: source.into(),
    })
}

impl Fetched {
    /// The lock entry for a copy of these files at `key`.
    pub(super) fn build_unit(
        &self,
        kind: Kind,
        key: PathBuf,
        folder: PathBuf,
    ) -> Result<Unit, AdoptionError> {
        let files = self
            .files
            .iter()
            .map(|(path, file)| Ok((path.clone(), kind.hash(path, &file.bytes)?)))
            .collect::<Result<BTreeMap<_, _>, AdoptionError>>()?;
        Ok(Unit {
            commit: self.revision.commit.clone(),
            files,
            folder,
            key,
            origin: match kind {
                Kind::Contracts => Origin::Contract(self.name.clone()),
                Kind::Skills => Origin::Skill(self.name.clone()),
            },
            reference: self.revision.reference.clone(),
            source: self.source.clone(),
        })
    }

    fn find_missing_file(
        &self,
        installation: &Installation<'_>,
        prepared: PreparedFile<'_>,
    ) -> Result<Option<(PathBuf, File)>, AdoptionError> {
        let PreparedFile {
            file,
            path,
            rule,
            target,
        } = prepared;
        if rule == Rule::Named {
            drop(self.read_document(installation.kind, &target, file, installation.name)?);
        }
        match find_obstruction(installation.root, installation.folder, path)? {
            None => Ok(Some((target, file.clone()))),
            Some(found) if found == target => {
                if rule == Rule::ReadOnly
                    && hash_file(&read(installation.root, &target)?) != hash_file(&file.bytes)
                {
                    return Err(AdoptionError::ReadOnlyDiffers {
                        target,
                        release: self.release(),
                    });
                }
                Ok(None)
            },
            Some(found) => {
                Err(AdoptionError::InTheWayOfAdd {
                    found,
                    target,
                    release: self.release(),
                })
            },
        }
    }

    /// Prepare the complete ordered write list under the destination's installation policy.
    pub(super) fn prepare_files(
        &self,
        installation: &Installation<'_>,
    ) -> Result<Vec<(PathBuf, File)>, AdoptionError> {
        let mut writes = Vec::new();
        for (path, file) in &self.files {
            let prepared = PreparedFile {
                file,
                path,
                rule: installation.kind.derive_rule(path),
                target: installation.folder.join(path),
            };
            let candidate = match installation.mode {
                InstallationMode::Existing => self.find_missing_file(installation, prepared)?,
                InstallationMode::New => Some(self.rename_file(installation, prepared)?),
            };
            if let Some(written) = candidate {
                writes.push(written);
            }
        }
        Ok(writes)
    }

    /// The upstream CONTRACT.md or SKILL.md renamed to `name`, refused unless this Tenet reads it
    /// at `target`.
    fn read_document(
        &self,
        kind: Kind,
        target: &Path,
        file: &File,
        name: &str,
    ) -> Result<File, AdoptionError> {
        let document = with_name(decode_text(&file.bytes, target)?, name)?;
        // Report the upstream text's error as a message, since it does not point into a local file.
        kind.validate(target, &document).map_err(|error| {
            AdoptionError::CannotInstall {
                release: self.release(),
                error: Box::new(error),
            }
        })?;
        Ok(File {
            bytes: document.into_bytes(),
            executable: file.executable,
        })
    }

    /// The library entry and release, as `SOURCE/NAME REF`.
    fn release(&self) -> String {
        format!("{}/{} {}", self.source, self.name, self.revision.reference)
    }

    fn rename_file(
        &self,
        installation: &Installation<'_>,
        prepared: PreparedFile<'_>,
    ) -> Result<(PathBuf, File), AdoptionError> {
        let PreparedFile {
            file,
            path,
            rule,
            target,
        } = prepared;
        if let Some(found) = find_obstruction(installation.root, installation.folder, path)? {
            return Err(AdoptionError::Exists(found));
        }
        let written = match rule {
            Rule::Named => {
                self.read_document(installation.kind, &target, file, installation.name)?
            },
            Rule::Merged | Rule::ReadOnly => file.clone(),
        };
        Ok((target, written))
    }
}

#[derive(Clone, Copy)]
pub(super) enum InstallationMode {
    Existing,
    New,
}

pub(super) struct Installation<'target> {
    folder: &'target Path,
    kind: Kind,
    mode: InstallationMode,
    name: &'target str,
    root: &'target Path,
}

impl<'target> Installation<'target> {
    pub(super) const fn new(
        root: &'target Path,
        kind: Kind,
        folder: &'target Path,
        name: &'target str,
        mode: InstallationMode,
    ) -> Self {
        Self {
            folder,
            kind,
            mode,
            name,
            root,
        }
    }
}

struct PreparedFile<'source> {
    file: &'source File,
    path: &'source Path,
    rule: Rule,
    target: PathBuf,
}
