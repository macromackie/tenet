//! Installing shared contracts and skills through `.tenet.lock`, and taking their upstream changes.
//!
//! Checking reads only the lock's separate `projects`, through discovery. Only these operations
//! touch the network, and nothing persists outside the project.

extern crate alloc;

mod add;
mod catalog;
mod commands;
mod error;
mod kind;
mod lock;
mod model;
mod unit;
mod upstream;
mod workspace;

pub use add::{AddRequest, Destination, SkillDestination, SkillRequest, add, add_skill};
pub use catalog::{Candidate, Catalog, find_candidates};
pub use commands::{diff, list_outdated, remove, update, validate_lock};
pub use error::AdoptionError;
pub use kind::{HashError, Kind, LocalState};
pub use model::{
    Added,
    Change,
    Comparison,
    FileComparison,
    Newest,
    Origin,
    Outcome,
    Recorded,
    Status,
    Updated,
    Version,
};
pub use tenet_engine::LockFileError;
