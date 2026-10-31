mod contracts;
mod error;
mod model;
mod paths;
mod selection;

pub use contracts::discover_contracts;
use contracts::read_contracts;
pub use error::DiscoveryError;
pub use model::{Coverage, ExcludedPath, Exclusion, Plan, Selection};
pub(crate) use paths::{is_allowed, is_contract, list_paths};
pub(crate) use selection::is_governing;
pub use selection::plan;
