//! Executable behavior checks, local project checkers, qualification, and safe native lint fixes.

#![expect(
    clippy::multiple_crate_versions,
    reason = "SHA-256 and BLAKE3 pin different cpufeatures versions listed in deny.toml."
)]

extern crate alloc;

pub mod behavior;
pub mod fixes;
pub mod mechanical;
