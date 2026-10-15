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
            "Does this change introduce, extend, or worsen a violation of an applicable rule in the contract? Compare the resulting contents in files with the change's before contents, using the patch and paths, and follow the resulting execution order. A violation already present before the change, which the change keeps without adding to it, does not count. Another instance, a new use, a copy, or a wider form of it does, and so does a violation created by moving, reordering, re-indenting, or deleting code. An added file is entirely new. Null contents in files means the file was deleted. Match means the change introduces, extends, or worsens a violation; no_match means it does not."
        }
        (Mode::Full, _) => {
            "Does the resulting file violate the contract? Match means a supported violation. No match means the file satisfies the applicable requirements, including when a conditional rule has no applicable implementation. There is no baseline compliance assumption."
        }
    };
    let mut query = format!(
        "{question}\nUse uncertain when necessary evidence is missing. Passing relevant arguments to an opaque helper does not establish its behavior; require its implementation or an explicit contract guarantee. Read the contract's scope and exceptions. Behavioral rules constrain a named feature if present; they do not require adding an absent feature. Existence requirements must be explicit. Checks are reviewer guidance, not executed tests. A value of {{\"omitted\": reason}} in place of contents, before, or patch was not supplied; judge from what is supplied and use uncertain if the omitted text is needed."
    );
    if stage != Stage::Applicability {
        query.push_str(
            "\nFor code, follow actual control flow, including indentation, block exit, and failure paths. Use dependency semantics stated in the contract.",
        );
    }
    if stage == Stage::Completeness {
        query.push_str(
            "\nAssess the requested_scope using all supplied files together. For full_contract, assess the whole contract scope, including missing required files and cross-file behavior. For selected_subjects, assess only violations in selected_files; support_files provide evidence and do not expand the claim. Inventory lists the contract's scoped files; omitted_source lists contents not supplied, and omitted_source_counts counts them when their paths are not listed. An unselected file's violation alone is outside a selected_subjects claim. Use uncertain if a necessary implementation is omitted or opaque; distinguish that from a feature shown to be absent from complete source.",
        );
    }
    query
}
