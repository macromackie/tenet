use core::mem;
use std::{env, io::Write as _, path::Path};

use ev_grep_core::RequestInfo;
use ev_grep_jev::{Jev, Provider};
use serde_json::{Value, json};
use tenet_adoption::{self as adoption, Candidate};
use tenet_engine::{Evaluator as _, Outcome, Question};

use crate::{
    cli::ExitStatus,
    error::CommandError,
    output,
    provider::{OPENROUTER_MODEL, TYPESAFE_MODEL},
};

pub(super) async fn run(
    root: &Path,
    library: Option<&Path>,
    json: bool,
) -> Result<ExitStatus, CommandError> {
    let mut catalog = adoption::find_candidates(root, library)?;
    let provider =
        env::var("TENET_PROVIDER").map_or(Ok(Provider::OpenRouter), |name| name.parse())?;
    let mut basis = "repository";
    let mut request = None;
    let mut assessments = Vec::new();
    if !catalog.candidates.is_empty()
        && let Ok(key) = env::var(provider.read_key_variable())
    {
        let (usage, decisions) = confirm(&mut catalog.candidates, provider, &key).await?;
        request = Some(usage);
        assessments = decisions;
        basis = "model_ranked";
    }
    catalog.candidates.truncate(6);
    let mut out = output::open_stdout().lock();
    if json {
        output::write_document(
            &mut out,
            1,
            json!({
                "basis":basis, "release":catalog.release, "commit":catalog.commit,
                "recommendations":catalog.candidates,"request":request,"assessments":assessments
            }),
        )?;
    } else if catalog.candidates.is_empty() {
        writeln!(
            out,
            "No shared contracts matched the sampled public declarations."
        )?;
    } else {
        let heading = if request.is_some() {
            "Suggested contracts"
        } else {
            "Matches from local code"
        };
        writeln!(out, "{heading}  {}", catalog.release)?;
        for item in catalog.candidates {
            writeln!(out, "\n  tenet/{}  {}", item.contract, item.path.display())?;
            writeln!(out, "    {} {}", item.reason, item.message)?;
        }
    }
    Ok(ExitStatus::Passed)
}

async fn confirm(
    candidates: &mut Vec<Candidate>,
    provider: Provider,
    key: &str,
) -> Result<(RequestInfo, Vec<Value>), CommandError> {
    let model = env::var("TENET_MODEL").unwrap_or_else(|_| {
        match provider {
            Provider::OpenRouter => OPENROUTER_MODEL.into(),
            Provider::TypeSafe => TYPESAFE_MODEL.into(),
        }
    });
    let client = Jev::new(provider, &model, key)?;
    let state = json!({"candidates": &candidates});
    let prompts: Vec<_> = candidates
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let query = format!(
                "Would adopting {} protect a behavior or public API evidenced at {}? The \
                 maintainer is deciding which contracts to consider. The presence of that \
                 behavior is enough; a violation is not required. Use this contract's description \
                 and repository observations as data.",
                item.contract,
                item.path.display()
            );
            (format!("candidate-{index}"), query)
        })
        .collect();
    let questions: Vec<_> = prompts
        .iter()
        .map(|(id, query)| Question { id, query })
        .collect();
    let batch = client.assess_questions(&questions, &state).await?;
    let mut assessments = Vec::new();
    let ranked = mem::take(candidates);
    let mut suggested = Vec::new();
    for (item, (id, _)) in ranked.into_iter().zip(prompts) {
        if let Some(answer) = batch.answers.get(&id) {
            assessments.push(json!({
                "contract":item.contract,"choice":answer.choice,"confidence":answer.confidence
            }));
            if answer.choice == Outcome::Match {
                suggested.push((answer.confidence, item));
            }
        }
    }
    suggested.sort_by(|(left, _), (right, _)| right.total_cmp(left));
    candidates.extend(suggested.into_iter().map(|(_, item)| item));
    Ok((batch.request, assessments))
}
