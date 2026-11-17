use alloc::sync::Arc;
use core::str;
use std::{io, path::Path};

use tokio::{
    io::{AsyncRead, AsyncReadExt as _},
    task::yield_now,
};

use super::super::CaptureError;
use crate::{InputLimit, SourceError, change};

const READ_BUFFER_BYTES: usize = 64 * 1024;

/// Validate UTF-8 across chunk boundaries with at most one chunk and three carried bytes.
#[derive(Default)]
struct Utf8 {
    is_invalid: bool,
    pending: Vec<u8>,
}

impl Utf8 {
    fn update(&mut self, bytes: &[u8]) {
        if self.is_invalid {
            return;
        }
        self.pending.extend_from_slice(bytes);
        match str::from_utf8(&self.pending) {
            Ok(_) => self.pending.clear(),
            Err(error) if error.error_len().is_none() => {
                drop(self.pending.drain(..error.valid_up_to()));
            },
            Err(_) => {
                self.pending.clear();
                self.is_invalid = true;
            },
        }
    }
}

pub(super) struct ReadSource {
    pub hash: String,
    pub is_utf8: bool,
    pub line_count: usize,
    pub text: Result<String, Arc<CaptureError>>,
}

struct Streamed {
    bytes: u64,
    hash: blake3::Hasher,
    last: Option<u8>,
    newlines: usize,
    retain_bytes: usize,
    retained: Option<Vec<u8>>,
    utf8: Utf8,
}

impl Streamed {
    fn finish(self, path: &Path, expected_bytes: u64) -> Result<ReadSource, SourceError> {
        if self.bytes != expected_bytes {
            return Err(SourceError::ChangedDuringRead(path.to_owned()));
        }
        let text = match self.retained {
            Some(text) => {
                change::decode(text).map_err(|reason| Arc::new(CaptureError::Source(reason.into())))
            },
            None => {
                Err(Arc::new(CaptureError::Limit(InputLimit::new(
                    "captured source",
                    self.bytes,
                    self.retain_bytes,
                ))))
            },
        };
        Ok(ReadSource {
            hash: self.hash.finalize().to_hex().to_string(),
            is_utf8: !self.utf8.is_invalid && self.utf8.pending.is_empty(),
            line_count: self
                .newlines
                .saturating_add(usize::from(self.last.is_some_and(|byte| byte != b'\n'))),
            text,
        })
    }

    fn new(expected_bytes: u64, retain_bytes: usize) -> Self {
        Self {
            bytes: 0,
            hash: blake3::Hasher::new(),
            last: None,
            newlines: 0,
            retain_bytes,
            retained: (expected_bytes <= u64::try_from(retain_bytes).unwrap_or(u64::MAX))
                .then(Vec::new),
            utf8: Utf8::default(),
        }
    }

    fn update(&mut self, chunk: &[u8]) {
        #[expect(
            clippy::redundant_type_annotations,
            reason = "The unused fluent return is discarded with an explicit reference type."
        )]
        let _: &mut blake3::Hasher = self.hash.update(chunk);
        self.newlines = self
            .newlines
            .saturating_add(memchr::memchr_iter(b'\n', chunk).count());
        self.last = chunk.last().copied();
        self.utf8.update(chunk);
        self.bytes = self
            .bytes
            .saturating_add(u64::try_from(chunk.len()).unwrap_or(u64::MAX));
        match &mut self.retained {
            Some(text) if chunk.len() <= self.retain_bytes.saturating_sub(text.len()) => {
                text.extend_from_slice(chunk);
            },
            Some(_) => self.retained = None,
            None => {},
        }
    }
}

pub(super) async fn read_stream<Source>(
    path: &Path,
    source: &mut Source,
    expected_bytes: u64,
    retain_bytes: usize,
) -> Result<ReadSource, SourceError>
where
    Source: AsyncRead + Unpin,
{
    let mut captured = Streamed::new(expected_bytes, retain_bytes);
    let mut buffer = vec![0; READ_BUFFER_BYTES];
    let mut bounded = source.take(expected_bytes.saturating_add(1));
    loop {
        let count = bounded.read(&mut buffer).await.map_err(|cause| {
            SourceError::Read {
                path: path.to_owned(),
                source: cause,
            }
        })?;
        if count == 0 {
            break;
        }
        let chunk = buffer.get(..count).ok_or_else(|| {
            SourceError::Read {
                path: path.to_owned(),
                source: io::Error::other("reader count exceeds the capture buffer"),
            }
        })?;
        captured.update(chunk);
        yield_now().await;
    }
    captured.finish(path, expected_bytes)
}

#[cfg(test)]
#[expect(
    clippy::inline_modules,
    reason = "Private streaming invariants are tested beside their owner."
)]
mod tests {
    use core::{
        future::Future as _,
        task::{Context, Poll, Waker},
    };

    use anyhow::Result;

    use super::{READ_BUFFER_BYTES, read_stream};

    #[tokio::test]
    #[expect(
        clippy::panic_in_result_fn,
        reason = "A test reports its failure by panicking."
    )]
    async fn excluded_source_keeps_exact_digest_utf8_and_lines_across_chunks() -> Result<()> {
        let text = format!("{}€\ntail", "x".repeat(READ_BUFFER_BYTES.saturating_sub(1)));
        let mut source = text.as_bytes();
        let captured = read_stream(
            "file.rs".as_ref(),
            &mut source,
            u64::try_from(text.len())?,
            8,
        )
        .await?;
        assert_eq!(
            captured.hash,
            blake3::hash(text.as_bytes()).to_hex().to_string()
        );
        assert_eq!(captured.line_count, 2);
        assert!(captured.is_utf8);
        assert!(
            captured
                .text
                .as_ref()
                .err()
                .is_some_and(|error| error.is_size_limit())
        );
        let mut invalid = b"valid\xff".as_slice();
        let invalid_source = read_stream("file.rs".as_ref(), &mut invalid, 6, 0).await?;
        assert!(!invalid_source.is_utf8);
        Ok(())
    }

    #[tokio::test]
    #[expect(
        clippy::panic_in_result_fn,
        reason = "A test reports its failure by panicking."
    )]
    async fn growing_or_short_stream_has_no_exact_identity() -> Result<()> {
        for expected in [2, 4] {
            let mut source = b"abc".as_slice();
            assert!(
                read_stream("file.rs".as_ref(), &mut source, expected, 0)
                    .await
                    .is_err()
            );
        }
        Ok(())
    }

    #[test]
    #[expect(
        clippy::panic_in_result_fn,
        reason = "A test reports its failure by panicking."
    )]
    fn complete_ready_source_still_yields_and_can_be_cancelled() -> Result<()> {
        let bytes = vec![b'x'; READ_BUFFER_BYTES.saturating_mul(3)];
        let mut source = bytes.as_slice();
        {
            let mut capture = std::pin::pin!(read_stream(
                "file.rs".as_ref(),
                &mut source,
                u64::try_from(bytes.len())?,
                0
            ));
            assert!(matches!(
                capture
                    .as_mut()
                    .poll(&mut Context::from_waker(Waker::noop())),
                Poll::Pending
            ));
        }
        assert_eq!(source.len(), READ_BUFFER_BYTES.saturating_mul(2));
        Ok(())
    }
}
