use core::{error, fmt};

/// An error followed by each of its causes, joined by ": ", as results, the review log and the
/// command line's error report show it.
#[derive(Debug)]
pub struct Causes<'error>(pub &'error (dyn error::Error + 'static));

impl fmt::Display for Causes<'_> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)?;
        let mut cause = self.0.source();
        while let Some(error) = cause {
            write!(f, ": {error}")?;
            cause = error.source();
        }
        Ok(())
    }
}
