use alloc::collections::BTreeMap;
use std::path::PathBuf;

use super::{ContextError, SpanRequest, is_source};
use crate::{Plan, change::Excerpt};

impl Plan {
    /// Supply repository files as context, each once. A file named without lines is supplied
    /// whole; otherwise only every range named for it. Lines the file no longer holds make it a
    /// capture error, so recapturing an earlier check shows its input changed; a new check or
    /// export refuses them with [`Plan::admit_context`].
    #[inline]
    pub fn with_context(mut self, requests: &[SpanRequest]) -> Result<Self, ContextError> {
        let mut ranges: BTreeMap<PathBuf, Vec<[usize; 2]>> = (self.excerpts.iter())
            .map(|(path, excerpt)| (path.clone(), excerpt.lines.clone()))
            .collect();
        for request in requests {
            let path = &request.path;
            if !is_source(&self.root, path).map_err(ContextError::Inspect)? {
                return Err(ContextError::NotSource(path.clone()));
            }
            let known = self.context.iter().any(|change| &change.path == path);
            match request.lines {
                None => {
                    drop(ranges.remove(path));
                },
                // A file already supplied whole stays whole.
                Some(_) if known && !ranges.contains_key(path) => {},
                Some((start, end)) => {
                    ranges
                        .entry(path.clone())
                        .or_default()
                        .push((start, end).into());
                },
            }
            if known {
                continue;
            }
            self.context.push(self.read_change(path)?);
        }
        self.excerpts = (self.context.iter())
            .filter_map(|change| {
                let lines = ranges.remove(&change.path)?;
                let excerpt = Excerpt::new(change, lines.clone())
                    .unwrap_or_else(|error| Excerpt::new_unavailable(lines, error));
                Some((change.path.clone(), excerpt))
            })
            .collect();
        Ok(self)
    }
}
