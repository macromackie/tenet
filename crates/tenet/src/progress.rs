use core::time::Duration;
use std::io;

use console::Term;

use crate::output;

#[derive(Default)]
pub(crate) struct Progress {
    lines: usize,
}

#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Methods follow the order the reporter calls them."
)]
impl Progress {
    #[expect(
        clippy::arithmetic_side_effects,
        clippy::indexing_slicing,
        reason = "The modulo keeps the frame index below the frame count."
    )]
    #[expect(
        clippy::as_conversions,
        reason = "The converted values are request budgets, byte caps, durations and fixture \
                  positions that fit the target type."
    )]
    #[expect(
        clippy::integer_division,
        reason = "The division rounds down by design: whole spinner frames and whole minutes, \
                  hours and days."
    )]
    #[expect(
        clippy::integer_division_remainder_used,
        reason = "The quotient and remainder pick a spinner frame or a whole time unit, and \
                  tokio::select! expands to a remainder for its branch order."
    )]
    pub(crate) fn draw(
        &mut self,
        term: &Term,
        active: &str,
        summary: &str,
        elapsed: Duration,
    ) -> io::Result<()> {
        const FRAMES: [char; 10] = [
            '\u{280b}', '\u{2819}', '\u{2839}', '\u{2838}', '\u{283c}', '\u{2834}', '\u{2826}',
            '\u{2827}', '\u{2807}', '\u{280f}',
        ];
        let frame = FRAMES[(elapsed.as_millis() / 100 % FRAMES.len() as u128) as usize];
        let spinner = output::format_styled(frame).yellow();
        let (height, width) = term.size();
        let mut lines = vec![if active.is_empty() {
            String::new()
        } else {
            format!(" {spinner} {active}")
        }];
        if height >= 6 {
            lines.extend([
                String::new(),
                format!(" {summary}"),
                format!(" Duration   {:.1}s", elapsed.as_secs_f64()),
            ]);
        }
        self.clear(term)?;
        for line in &lines {
            term.write_line(&console::truncate_str(
                line,
                usize::from(width.saturating_sub(1)),
                "\u{2026}",
            ))?;
        }
        self.lines = lines.len();
        term.flush()
    }

    pub(crate) fn clear(&mut self, term: &Term) -> io::Result<()> {
        if self.lines > 0 {
            term.clear_last_lines(self.lines)?;
            self.lines = 0;
        }
        Ok(())
    }
}
