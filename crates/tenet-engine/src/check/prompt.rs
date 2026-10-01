use crate::{Mode, Stage};

pub(crate) fn query(stage: Stage, mode: Mode) -> String {
    let question = match (mode, stage) {
        (Mode::Diff, Stage::Applicability) => {
            "Could this change affect the contract? Assume the base satisfied it. Match means potentially affected, not broken. No match means clearly unaffected. Changes to indentation, block membership, execution order, deletions, and paths can affect behavior even when calls are unchanged."
        }
        (Mode::Full, Stage::Applicability) => {
            "Does the contract govern this file? Use the contract's stated scope; a contract may govern code, configuration, tests, or documentation. Match means potentially relevant, not broken. No match means clearly outside that scope. A related name or general-purpose helper alone does not establish that a scoped feature exists."
        }
        (Mode::Diff, _) => {
            "Does the current content violate an applicable rule in the contract? Assess the change against a compliant baseline. Use the change's before contents, patch, and paths to understand it; judge the resulting contents in files, not removed violations. Null contents in files means the file was deleted. Match means a violation, no_match means the applicable rules hold."
        }
        (Mode::Full, _) => {
            "Does the resulting file violate the contract? Match means a supported violation. No match means the file satisfies the applicable requirements, including when a conditional rule has no applicable implementation. There is no baseline compliance assumption."
        }
    };
    let mut query = format!(
        "{question}\nUse uncertain when necessary evidence is missing. Passing relevant arguments to an opaque helper does not establish its behavior; require its implementation or an explicit contract guarantee. Read the contract's scope and exceptions. Behavioral rules constrain a named feature if present; they do not require adding an absent feature. Existence requirements must be explicit. Checks are reviewer guidance, not executed tests."
    );
    if stage != Stage::Applicability {
        query.push_str(
            "\nFor code, follow actual control flow, including indentation, block exit, and failure paths. Use dependency semantics stated in the contract.",
        );
    }
    if stage == Stage::Completeness {
        query.push_str(
            "\nAssess the whole scoped repository, including missing required files and cross-file behavior. Inventory lists the scoped files; omitted_source lists contents not supplied. Use uncertain if a necessary implementation is omitted or opaque; distinguish that from a feature shown to be absent from complete source.",
        );
    }
    query
}
