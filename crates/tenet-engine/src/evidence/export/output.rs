use std::io::{self, Write};

pub(super) struct LimitedOutput {
    bytes: Vec<u8>,
    limit: usize,
}

impl Write for LimitedOutput {
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(io::Error::other(format!(
                "encoded evidence exceeds {:?} bytes; input was not truncated",
                self.limit
            )));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
}

impl LimitedOutput {
    #[must_use]
    pub(super) fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    #[must_use]
    pub(super) const fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::inline_modules,
    reason = "Unit tests stay beside the private functions they test."
)]
mod tests {
    use core::cell::Cell;

    use super::{super::Sequence, LimitedOutput};

    #[test]
    fn serialized_metadata_stops_at_the_output_bound() {
        let visited = Cell::new(0_u32);
        let paths = (0..1_000_000_u32).inspect(|_| visited.set(visited.get() + 1));
        let mut output = LimitedOutput {
            bytes: Vec::new(),
            limit: 32,
        };
        assert!(serde_json::to_writer(&mut output, &Sequence(paths)).is_err());
        assert!(output.bytes.len() <= 32);
        assert!(visited.get() < 20);
    }
}
