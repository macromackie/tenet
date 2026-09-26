use std::{io, time::Duration};

use console::Term;

#[derive(Default)]
pub(crate) struct Progress {
    lines: usize,
}

impl Progress {
    pub(crate) fn draw(
        &mut self,
        term: &mut Term,
        active: &str,
        summary: &str,
        elapsed: Duration,
    ) -> io::Result<()> {
        const FRAMES: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
        let frame = FRAMES[(elapsed.as_millis() / 100 % FRAMES.len() as u128) as usize];
        let spinner = console::style(frame).for_stderr().yellow();
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
                "…",
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
