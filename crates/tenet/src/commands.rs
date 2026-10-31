mod administration;
mod documents;
mod dry_run;
mod evaluate;
mod finish;
mod prepare;

use administration::{execute_contracts, execute_skills};
use documents::{export, validate};
use dry_run::report_dry_run;
use evaluate::evaluate;
use finish::finish;
use prepare::prepare;

use crate::{
    cli::{Cli, Command, ExitStatus},
    error::CommandError,
    review,
};

#[expect(
    clippy::future_not_send,
    reason = "The command drives this future on one current-thread runtime and never moves it \
              across threads."
)]
pub(crate) async fn execute(cli: &Cli) -> Result<ExitStatus, CommandError> {
    match &cli.command {
        Command::Validate => validate(&cli.root),
        Command::Skills { command } => execute_skills(&cli.root, command),
        Command::Contracts { command } => execute_contracts(&cli.root, command),
        Command::Check(check) => evaluate(&cli.root, &cli.resolve_state(), check).await,
        Command::Status {
            base,
            json,
            prune_cache,
        } => {
            review::report_status(
                &cli.root,
                &cli.resolve_state(),
                base.as_deref(),
                *json,
                *prune_cache,
            )
            .await
        },
        Command::Show {
            contract: Some(contract),
            paths,
            json,
            ..
        } => review::show(&cli.root, &cli.resolve_state(), contract, paths, *json).await,
        Command::Show {
            contract: None,
            json,
            ..
        } => review::show_pending(&cli.root, &cli.resolve_state(), *json).await,
        Command::Record(record) => review::record(&cli.root, &cli.resolve_state(), record).await,
        Command::Log { contract, json } => {
            review::log(&cli.resolve_state(), contract.as_deref(), *json)
        },
        Command::Export(configuration) => export(&cli.root, configuration).await,
    }
}
