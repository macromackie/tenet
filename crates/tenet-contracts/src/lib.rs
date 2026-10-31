//! Numbered Markdown requirements, installed skill documents, and the adoption lock.

extern crate alloc;

mod contract;
mod document;
mod lock;
mod markdown;
mod parse;
mod scope;
mod skill;

pub use contract::Contract;
pub use document::{DocumentError, DocumentProblem, FrontmatterError};
pub use lock::{
    DEFAULT_SOURCE,
    DEFAULT_SOURCE_URL,
    Entry,
    LOCK_FILE,
    Lock,
    LockError,
    SkillEntry,
    hash_contract,
    hash_file,
    hash_skill,
    is_contract_asset,
};
pub use parse::{find_conflict_marker, is_kebab, name_of, parse, parse_version, with_name};
pub use skill::{is_skill_name, validate_skill};
