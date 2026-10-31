use std::io;

use super::Reporter;
use crate::report_format::format_contract_counts;

impl Reporter<'_> {
    #[expect(
        clippy::excessive_nesting,
        reason = "Each level is a loop, match or closure over one parsed structure, and \
                  flattening it would split one decision."
    )]
    pub(crate) fn refresh(&mut self) -> io::Result<()> {
        if self.live && self.running {
            let current = self.contracts.iter().find_map(|contract| {
                self.active_contracts
                    .get(&contract.name)
                    .map(|progress| (&contract.name, progress))
            });
            let active = current
                .map(|(name, progress)| {
                    let phase = if progress.completing {
                        " \u{b7} assessing contract"
                    } else {
                        ""
                    };
                    let others = self.active_contracts.len().saturating_sub(1);
                    let extra = if others > 0 {
                        format!(" · {others} other contracts")
                    } else {
                        String::new()
                    };
                    format!(
                        "{name} {}/{} files{phase}{extra}",
                        progress.done, progress.total
                    )
                })
                .unwrap_or_default();
            self.progress.draw(
                &self.term,
                &active,
                &format!(
                    "Contracts  {}",
                    format_contract_counts(&self.contract_statuses, self.contracts.len())
                ),
                self.started.elapsed(),
            )?;
        }
        Ok(())
    }
}
