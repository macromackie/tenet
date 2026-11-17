use super::{ContextError, is_source};
use crate::{Plan, SpanRequest};

impl Plan {
    /// Declare supporting paths and ranges without reading source before bounded capture.
    #[inline]
    pub fn with_context(mut self, requests: &[SpanRequest]) -> Result<Self, ContextError> {
        for request in requests {
            let path = &request.path;
            if let Some((start, end)) = request.lines
                && !(1 <= start && start <= end)
            {
                return Err(crate::ExcerptError::OutsideFile {
                    path: path.clone(),
                    start,
                    end,
                    lines: 0,
                }
                .into());
            }
            if !is_source(&self.root, path).map_err(ContextError::Inspect)? {
                return Err(ContextError::NotSource(path.clone()));
            }
            let known = self.context.contains(path);
            match request.lines {
                None => {
                    drop(self.excerpts.remove(path));
                },
                Some(_) if known && !self.excerpts.contains_key(path) => {},
                Some((start, end)) => {
                    self.excerpts
                        .entry(path.clone())
                        .or_default()
                        .push((start, end).into());
                },
            }
            if !known {
                self.context.push(path.clone());
            }
        }
        for ranges in self.excerpts.values_mut() {
            ranges.sort_unstable();
            let mut merged: Vec<[usize; 2]> = Vec::new();
            for &[start, end] in &*ranges {
                match merged.last_mut() {
                    Some(last) if start <= last[1].saturating_add(1) => last[1] = last[1].max(end),
                    _ => merged.push([start, end]),
                }
            }
            *ranges = merged;
        }
        Ok(self)
    }
}

#[cfg(test)]
#[expect(
    clippy::inline_modules,
    reason = "Raw range normalization is tested beside its owner."
)]
mod tests {
    use std::fs;

    use anyhow::Result;

    use crate::{Selection, SpanRequest, plan};

    #[test]
    fn overlapping_valid_range_cannot_hide_a_reversed_public_request() -> Result<()> {
        let root = tempfile::tempdir()?;
        let document = root.path().join(".contracts/001-ranges");
        fs::create_dir_all(&document)?;
        fs::write(
            document.join("CONTRACT.md"),
            "---\nname: ranges\nmessage: Preserve source.\n---\n## Rules\nPreserve source.\n",
        )?;
        fs::write(root.path().join("file.rs"), "line\n".repeat(10))?;
        let selected = plan(root.path(), &Selection::default())?;
        let requests = [
            SpanRequest {
                path: "file.rs".into(),
                lines: Some((1, 10)),
            },
            SpanRequest {
                path: "file.rs".into(),
                lines: Some((5, 4)),
            },
        ];
        drop(selected.with_context(&requests).unwrap_err());
        Ok(())
    }
}
