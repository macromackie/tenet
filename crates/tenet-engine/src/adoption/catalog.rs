//! Match released catalog entries to concrete public declarations without executing project code.

mod surface;

use alloc::collections::{BTreeMap, BTreeSet};
use core::cmp::Reverse;
use std::{
    io,
    path::{Path, PathBuf},
};

use serde::Serialize;
use tenet_contracts::{Contract, DEFAULT_SOURCE_URL, is_kebab, parse};

use super::{AdoptionError, Upstream};
use crate::{
    discovery::{discover_contracts, list_paths},
    git,
    lock,
};

/// A contract and the repository evidence that led to its selection.
#[derive(Debug, Serialize)]
pub struct Candidate {
    /// Name of a contract in the selected library release.
    pub contract: String,
    /// The rule's short description from its released document.
    pub message: String,
    /// A real project-relative file with a matching public declaration.
    pub path: PathBuf,
    /// What Tenet found in that file.
    pub reason: String,
}

/// A released catalog's highest-scoring repository matches.
#[derive(Debug, Serialize)]
pub struct Catalog {
    /// Shortlisted matches in descending order.
    pub candidates: Vec<Candidate>,
    /// Exact Git commit of the catalog.
    pub commit: String,
    /// The selected release tag.
    pub release: String,
}

/// Inspect public source declarations and match against the released catalog.
#[inline]
pub fn find_candidates(root: &Path, library: Option<&Path>) -> Result<Catalog, AdoptionError> {
    let url = library
        .map(Path::canonicalize)
        .transpose()
        .map_err(AdoptionError::Scratch)?;
    let location = match url.as_deref() {
        Some(path) => path.to_str().ok_or(AdoptionError::NotUtf8Path)?,
        None => DEFAULT_SOURCE_URL,
    };
    let upstream = Upstream::download(location)?;
    let revision = upstream.resolve(None)?;
    let available = list_documents(&upstream, &revision.commit)?;
    Ok(Catalog {
        candidates: rank(root, &available)?,
        commit: revision.commit,
        release: revision.reference,
    })
}

fn rank(root: &Path, available: &[Contract]) -> Result<Vec<Candidate>, AdoptionError> {
    let inventory = list_paths(root)?;
    let surfaces = surface::inspect(root, &inventory)?;
    let local = discover_contracts(root)?;
    let installed = lock::read_lock(root)?;
    let adopted: Vec<_> = local
        .iter()
        .map(|contract| {
            let name = installed
                .contracts
                .get(&contract.path)
                .map_or(contract.name.as_str(), |entry| entry.contract.as_str());
            (contract, name)
        })
        .collect();
    let mut ranking = Vec::new();
    for contract in available {
        let best = surfaces
            .iter()
            .filter(|surface| {
                !adopted.iter().any(|(current, name)| {
                    *name == contract.name && current.is_governing(surface.path())
                })
            })
            .filter_map(|surface| {
                surface
                    .score(contract)
                    .map(|score| (Reverse(score), surface.path(), surface))
            })
            .min_by_key(|(score, path, _)| (*score, *path));
        if let Some((score, _, surface)) = best {
            ranking.push((
                score,
                Candidate {
                    contract: contract.name.clone(),
                    message: contract.message.clone(),
                    path: surface.path().to_owned(),
                    reason: surface.explain_match(),
                },
            ));
        }
    }
    ranking.sort_by(|(left_score, left), (right_score, right)| {
        (left_score, &left.contract).cmp(&(right_score, &right.contract))
    });
    Ok(ranking.into_iter().take(16).map(|(_, item)| item).collect())
}

const GIT_FILE_MODES: &[&str] = &["100644", "100755"];

fn list_documents(upstream: &Upstream, commit: &str) -> Result<Vec<Contract>, AdoptionError> {
    let listing = String::from_utf8(git::run_git(
        upstream.repository(),
        &["ls-tree", "-r", "-l", commit, "--", "contracts/"],
    )?)?;
    let mut entries = BTreeMap::new();
    let mut bytes = 0_usize;
    for line in listing.lines() {
        let (metadata, path) = line
            .split_once('\t')
            .ok_or(AdoptionError::MalformedListing)?;
        let Some(name) = path
            .strip_prefix("contracts/")
            .and_then(|rest| rest.strip_suffix("/CONTRACT.md"))
            .filter(|name| is_kebab(name))
        else {
            continue;
        };
        let fields: Vec<_> = metadata.split_whitespace().collect();
        let [mode, "blob", object, number] = fields.as_slice() else {
            return Err(AdoptionError::MalformedListing);
        };
        if !GIT_FILE_MODES.contains(mode) {
            return Err(AdoptionError::MalformedListing);
        }
        let size: usize = number
            .parse()
            .map_err(|error| AdoptionError::Scratch(io::Error::other(error)))?;
        bytes = bytes.saturating_add(size);
        if size > 0x0001_0000 || bytes > 8 * 1024 * 1024 || entries.len() >= 512 {
            return Err(AdoptionError::Scratch(io::Error::other(
                "contract catalog exceeds document or catalog limits",
            )));
        }
        let previous = entries.insert(name, *object);
        if previous.is_some() {
            return Err(AdoptionError::MalformedListing);
        }
    }
    read_documents(upstream, entries)
}

fn read_documents(
    upstream: &Upstream,
    entries: BTreeMap<&str, &str>,
) -> Result<Vec<Contract>, AdoptionError> {
    let mut contracts = Vec::new();
    for (name, object) in entries {
        let document = String::from_utf8(git::run_git(
            upstream.repository(),
            &["cat-file", "blob", object],
        )?)?;
        let path = Path::new(".contracts")
            .join(format!("001-{name}"))
            .join("CONTRACT.md");
        let contract = parse(&path, &document)?;
        if contract.name != name {
            return Err(AdoptionError::MalformedListing);
        }
        contracts.push(contract);
    }
    Ok(contracts)
}

fn split_words(text: &str) -> BTreeSet<String> {
    let mut split = String::new();
    let mut lower = false;
    for character in text.chars() {
        if (lower && character.is_ascii_uppercase()) || !character.is_ascii_alphanumeric() {
            split.push(' ');
        }
        if character.is_ascii_alphanumeric() {
            split.push(character.to_ascii_lowercase());
        }
        lower = character.is_ascii_lowercase();
    }
    split
        .split_ascii_whitespace()
        .filter(|word| word.len() > 2)
        .map(|word| {
            if word.len() > 4 && !word.ends_with("ss") {
                word.strip_suffix('s').unwrap_or(word)
            } else {
                word
            }
            .to_owned()
        })
        .collect()
}
