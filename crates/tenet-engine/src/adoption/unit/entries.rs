//! Lock entries and installed copies share one canonical representation.

use std::path::Path;

use tenet_contracts::{Entry, Lock, SkillEntry};

use super::{AdoptionError, Kind, Origin, Unit};

impl Unit {
    pub(in crate::adoption) fn find(
        lock: &Lock,
        kind: Kind,
        key: &Path,
    ) -> Result<Self, AdoptionError> {
        let unit = match kind {
            Kind::Contracts => {
                lock.contracts
                    .get(key)
                    .map(|entry| Self::from_contract(key, entry))
                    .transpose()?
            },
            Kind::Skills => {
                lock.skills
                    .get(key)
                    .map(|entry| Self::from_skill(key, entry))
            },
        };
        unit.ok_or_else(|| AdoptionError::NotRecorded(key.to_owned()))
    }

    fn from_contract(path: &Path, entry: &Entry) -> Result<Self, AdoptionError> {
        let mut files = entry.files.clone();
        drop(files.insert(Kind::Contracts.name_document().into(), entry.hash.clone()));
        Ok(Self {
            commit: entry.commit.clone(),
            files,
            folder: path
                .parent()
                .ok_or(AdoptionError::NoContractFolder)?
                .to_owned(),
            key: path.to_owned(),
            origin: Origin::Contract(entry.contract.clone()),
            reference: entry.reference.clone(),
            source: entry.source.clone(),
        })
    }

    fn from_skill(folder: &Path, entry: &SkillEntry) -> Self {
        Self {
            commit: entry.commit.clone(),
            files: entry.files.clone(),
            folder: folder.to_owned(),
            key: folder.to_owned(),
            origin: Origin::Skill(entry.skill.clone()),
            reference: entry.reference.clone(),
            source: entry.source.clone(),
        }
    }

    /// Every entry of `kind`, in key order.
    pub(in crate::adoption) fn list_all(
        lock: &Lock,
        kind: Kind,
    ) -> Result<Vec<Self>, AdoptionError> {
        match kind {
            Kind::Contracts => {
                lock.contracts
                    .iter()
                    .map(|(path, entry)| Self::from_contract(path, entry))
                    .collect()
            },
            Kind::Skills => {
                Ok(lock
                    .skills
                    .iter()
                    .map(|(folder, entry)| Self::from_skill(folder, entry))
                    .collect())
            },
        }
    }

    /// Record this copy in the lock, replacing any entry under its key.
    pub(in crate::adoption) fn store(self, lock: &mut Lock) -> Result<(), AdoptionError> {
        match self.origin {
            Origin::Contract(contract) => {
                let mut files = self.files;
                let hash = files
                    .remove(Path::new(Kind::Contracts.name_document()))
                    .ok_or(AdoptionError::NoContractDocument)?;
                drop(lock.contracts.insert(
                    self.key,
                    Entry {
                        source: self.source,
                        contract,
                        reference: self.reference,
                        commit: self.commit,
                        hash,
                        files,
                    },
                ));
            },
            Origin::Skill(skill) => {
                drop(lock.skills.insert(
                    self.key,
                    SkillEntry {
                        source: self.source,
                        skill,
                        reference: self.reference,
                        commit: self.commit,
                        files: self.files,
                    },
                ));
            },
        }
        Ok(())
    }
}
