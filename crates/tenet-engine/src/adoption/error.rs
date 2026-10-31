//! Why an adoption command refuses or fails, each with the message it reports.

use alloc::string::FromUtf8Error;
use core::str::Utf8Error;
use std::{io, path::PathBuf};

use tenet_contracts::{DocumentError, FrontmatterError, LOCK_FILE};

use super::{HashError, Kind, LockFileError};
use crate::{DiscoveryError, GitError};

/// Why an adoption command refuses or fails.
#[derive(Debug, thiserror::Error)]
pub enum AdoptionError {
    #[error("{:?} is already recorded in {LOCK_FILE:?}", .0.display())]
    AlreadyRecorded(PathBuf),
    /// The upstream document's error, reported as a message because it does not point into a
    /// local file.
    #[error("cannot install {release:?}: {error:#}")]
    CannotInstall {
        release: String,
        error: Box<DocumentError>,
    },
    #[error("{:?}:{line:?}: unresolved merge conflict marker", .file.display())]
    ConflictMarker { file: PathBuf, line: usize },
    #[error(transparent)]
    Discovery(#[from] DiscoveryError),
    #[error(transparent)]
    Document(#[from] DocumentError),
    #[error(
        // The message names a command to copy, whose subcommand and local name are bare words.
        // ast-grep-ignore: diagnostic-values-rust
        "{:?} is missing; restore it, fix its path in {LOCK_FILE:?}, or run \
         `tenet {} remove {:?}`",
        .document.display(),
        .kind.name(),
        .key.display()
    )]
    DocumentMissing {
        document: PathBuf,
        kind: Kind,
        key: PathBuf,
    },
    #[error(
        // The message names a command to copy, whose subcommand and local name are bare words.
        // ast-grep-ignore: diagnostic-values-rust
        "{:?} does not match its hash in {LOCK_FILE:?}, and Tenet never overwrites a read-only \
         file; compare it with upstream using `tenet {} diff {:?}`, restore it, and keep your \
         changes in a file of your own",
        .file.display(),
        .kind.name(),
        .key.display()
    )]
    EditedReadOnly {
        file: PathBuf,
        kind: Kind,
        key: PathBuf,
    },
    #[error("{:?} already exists", .0.display())]
    Exists(PathBuf),

    #[error(transparent)]
    Frontmatter(#[from] FrontmatterError),
    #[error(transparent)]
    Git(#[from] GitError),
    #[error(transparent)]
    Hash(#[from] HashError),
    #[error("cannot hash {:?}", .path.display())]
    HashFile { path: PathBuf, source: HashError },
    #[error(
        "Git ignores {path:?} through {rule:?}; change that rule or install the copy elsewhere"
    )]
    Ignored { path: String, rule: String },
    #[error(
        "{:?} is in the way of {:?}, which {release:?} has; move it aside and add again",
        .found.display(),
        .target.display()
    )]
    InTheWayOfAdd {
        found: PathBuf,
        target: PathBuf,
        release: String,
    },
    #[error(
        "{:?} is in the way of {:?}, which {reference:?} adds; move it aside and update again",
        .found.display(),
        .path.display()
    )]
    InTheWayOfUpdate {
        found: PathBuf,
        path: PathBuf,
        reference: String,
    },
    #[error("cannot inspect {:?}", .path.display())]
    Inspect { path: PathBuf, source: io::Error },
    #[error("{path:?} must be a regular file in {url:?}")]
    IrregularFile { path: String, url: String },
    #[error("library path is not UTF-8")]
    LibraryPathNotUtf8(#[source] Utf8Error),

    #[error(transparent)]
    LockFile(#[from] LockFileError),
    #[error("malformed Git listing")]
    MalformedListing,
    #[error("Git merge-file failed: {0:#}")]
    MergeFailed(String),
    #[error("merged document is not UTF-8")]
    MergedNotUtf8(#[source] FromUtf8Error),
    #[error("the locked commit {commit:?} has no {name:?}")]
    MissingAtLock { commit: String, name: String },
    #[error(
        // The message names a command to copy, whose subcommand and local name are bare words.
        // ast-grep-ignore: diagnostic-values-rust
        "{:?} is recorded in {LOCK_FILE:?} but missing; restore it from Git, and compare it with \
         upstream using `tenet {} diff {:?}`",
        .file.display(),
        .kind.name(),
        .key.display()
    )]
    MissingFile {
        file: PathBuf,
        kind: Kind,
        key: PathBuf,
    },
    #[error(
        // The message names a command to copy, whose subcommand and local name are bare words.
        // ast-grep-ignore: diagnostic-values-rust
        "contract name {name:?} is already used; choose another with --name \
         {scope}-{library_name}"
    )]
    NameUsed {
        name: String,
        scope: String,
        library_name: String,
    },
    #[error(
        // The message names a command to copy, whose subcommand and local name are bare words.
        // ast-grep-ignore: diagnostic-values-rust
        "tenet {} needs a Git checkout",
        .kind.name()
    )]
    NeedsCheckout { kind: Kind, source: GitError },
    #[error("this command needs a Git checkout")]
    NeedsGit(#[source] GitError),
    #[error("a contract's files must include its CONTRACT.md")]
    NoContractDocument,
    #[error("contract has no folder")]
    NoContractFolder,
    #[error("{:?} has no {document:?}", .key.display())]
    NoDocument {
        key: PathBuf,
        document: &'static str,
    },
    #[error("installed file has no folder")]
    NoInstalledFolder,
    #[error("library copy has no {0:?}")]
    NoLibraryDocument(&'static str),

    #[error("{0:?} has no vX.Y.Z tags; name a reference with SOURCE/NAME@REF")]
    NoReleases(String),
    #[error("skill has no folder")]
    NoSkillFolder,
    #[error("skill has no folder name")]
    NoSkillFolderName,
    #[error("{library:?} does not exist at {reference:?}")]
    NotInLibrary { library: String, reference: String },
    #[error("{:?} is not a contract in this project", .0.display())]
    NotProjectContract(PathBuf),

    #[error("{:?} is not recorded in {LOCK_FILE:?}", .0.display())]
    NotRecorded(PathBuf),
    #[error("{:?} must be relative to the project root", .0.display())]
    NotRelative(PathBuf),
    #[error("path is not UTF-8")]
    NotUtf8Path,
    #[error("{:?} is not UTF-8", .path.display())]
    NotUtf8Text { path: PathBuf, source: Utf8Error },
    #[error("nothing is recorded in {LOCK_FILE:?}")]
    NothingRecorded,
    #[error("contract number {number:?} is already used in {:?}", .scope.display())]
    NumberUsed { number: u32, scope: PathBuf },
    #[error("Git reference cannot start with '-'")]
    OptionReference,
    #[error("source URL cannot start with '-'")]
    OptionUrl,

    #[error(transparent)]
    OutputText(#[from] FromUtf8Error),
    #[error("cannot write {LOCK_FILE:?}")]
    PersistLock(#[source] tempfile::PersistError),
    #[error("cannot read {:?}", .path.display())]
    Read { path: PathBuf, source: io::Error },
    #[error("cannot read {:?}; install a new copy with --dir", .path.display())]
    ReadExistingSkill { path: PathBuf, source: io::Error },
    #[error("cannot fetch contract library {url:?}")]
    ReadLibrary { url: String, source: GitError },
    #[error(
        "{:?} differs from {release:?}; Tenet installs it read-only, so restore the released file \
         before recording this copy",
        .target.display()
    )]
    ReadOnlyDiffers { target: PathBuf, release: String },
    #[error("cannot remove {:?}", .path.display())]
    Remove { path: PathBuf, source: io::Error },
    #[error(transparent)]
    RenderLock(#[from] serde_json::Error),
    #[error("scope {:?} is not a directory", .0.display())]
    ScopeNotDirectory(PathBuf),
    #[error(transparent)]
    Scratch(io::Error),
    #[error("{:?} already exists; record it with --path {:?}", .0.display(), .0.display())]
    SkillExists(PathBuf),
    #[error(
        "skill name {0:?} must use lowercase letters, digits, and single hyphens, at most 64 \
         characters"
    )]
    SkillName(String),
    #[error("source alias must be lowercase kebab-case")]
    SourceAlias,
    #[error("--source expects ALIAS=URL")]
    SourceFormat,
    #[error(
        "source {alias:?} already names {existing:?}; register {url:?} under another alias, as in \
         --source local={url:?} with local/{name:?}"
    )]
    SourceTaken {
        alias: String,
        existing: String,
        url: String,
        name: String,
    },

    #[error("invalid URL for source {0:?}")]
    SourceUrl(String),
    #[error("expected SOURCE/NAME[@REF], such as tenet/data-and-apis")]
    SpecFormat,
    #[error("expected SOURCE/NAME[@REF] with kebab-case names")]
    SpecNames,
    #[error(
        "commit or set aside changes to {:?} first, or keep its files with --keep-file",
        .0.display()
    )]
    UncommittedRemove(PathBuf),
    #[error("commit or set aside changes to {LOCK_FILE:?} and the selected {:?} first", .0.name())]
    UncommittedUpdate(Kind),
    #[error("{LOCK_FILE:?} names unknown source {0:?}")]
    UnknownLockSource(String),
    #[error("{reference:?} does not exist in {url:?}")]
    UnknownReference {
        reference: String,
        url: String,
        source: GitError,
    },
    #[error("unknown source {0:?}; register it with --source {0:?}=URL")]
    UnknownSource(String),
    /// The upstream document's error, reported as a message so it never points into the local
    /// file.
    #[error("{reference:?} {reason:?}: {error:#}")]
    UnreadableRelease {
        reference: String,
        reason: &'static str,
        error: Box<DocumentError>,
    },
    #[error(
        "{:?} is not recorded in {LOCK_FILE:?}, but {reference:?} adds a file there; move it \
         aside and update again",
        .path.display()
    )]
    UnrecordedAtTarget { path: PathBuf, reference: String },
    #[error(
        "{path:?} in {url:?} is neither CONTRACT.md nor a file under assets/<tool>/, so Tenet {:?} \
         cannot install it; the contract may need a newer Tenet",
        env!("CARGO_PKG_VERSION")
    )]
    UnsupportedFile { path: String, url: String },
    #[error("cannot write {:?}", .path.display())]
    Write { path: PathBuf, source: io::Error },
    #[error("cannot write {LOCK_FILE:?}")]
    WriteLock(#[source] io::Error),
}

impl AdoptionError {
    /// The refused local document, when this error is one.
    #[must_use]
    #[inline]
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "The remaining variants take no action here, and listing each would repeat the \
                  enum."
    )]
    pub const fn as_document(&self) -> Option<&DocumentError> {
        match self {
            Self::Document(error) => Some(error),
            Self::Discovery(error) => error.as_document(),
            _ => None,
        }
    }
}
