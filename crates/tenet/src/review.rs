//! Commands that read and extend the review kept in `.tenet/`: status, show, record, and log.

mod brief;
mod display;
mod files;
mod history;
mod mutations;
mod show;
mod status;

use brief::write_brief;
use display::{
    check_command,
    find_model,
    format_ago,
    format_paths_note,
    format_row_name,
    format_selection_text,
    format_span_text,
    format_state_text,
    format_tally,
    format_threshold_note,
};
use files::write_files;
pub(crate) use mutations::{log, record};
use serde_json::Value;
pub(crate) use show::{show, show_pending};
pub(crate) use status::report_status;
use tenet_engine::{
    Status,
    review::{Assessment, Record},
};

/// The schema version of the object that `status --json` prints.
const STATUS_VERSION: u64 = 1;
/// The schema version of the objects that `show NAME --json` and `show --pending --json` print.
const SHOW_VERSION: u64 = 2;
/// The schema version of each review log entry that `log --json` prints.
const LOG_VERSION: u64 = 2;

/// How many records and earlier assessments `show` lists; `log NAME` lists every one.
const HISTORY: usize = 5;
/// `show` lists every file and file result of a scope this size or smaller, and otherwise only
/// those that need attention.
const LISTED: usize = 20;
/// The results that need no attention.
const SETTLED_STATUSES: [Status; 2] = [Status::Pass, Status::NotApplicable];
/// The change kinds a long list leaves out.
const ROUTINE_KINDS: [&str; 2] = ["selected", "modified"];
/// How many unreadable files `show` names; it counts the rest.
const UNREADABLE: usize = 5;
/// How many changed ranges `show` offers as evidence.
const CITATIONS: usize = 10;

/// A contract's records and earlier assessments, newest first, and what changed in its inputs
/// since the latest earlier assessment.
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Fields follow the order the history shows them."
)]
struct History<'review> {
    records: Vec<&'review Record>,
    earlier: Vec<&'review Assessment>,
    changes_since: Vec<Value>,
}
