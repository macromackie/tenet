use alloc::fmt;
use core::str::FromStr;
use std::path::PathBuf;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

/// A `PATH:START-END` request whose lines are not a range counted from 1.
#[derive(Debug, thiserror::Error)]
#[error("invalid line range in {0:?}; use PATH:START-END, counting lines from 1")]
pub struct SpanError(String);

#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Field order is the derived ordering."
)]
/// A repository file, whole or one inclusive range of lines counted from 1, as `--context` and
/// `record --evidence` name it: `PATH`, `PATH:LINE`, or `PATH:START-END`.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SpanRequest {
    pub path: PathBuf,
    /// Inclusive, 1-based; absent for the whole file.
    pub lines: Option<(usize, usize)>,
}

impl FromStr for SpanRequest {
    type Err = SpanError;

    #[inline]
    fn from_str(text: &str) -> Result<Self, SpanError> {
        if let Some((path, lines)) = text.rsplit_once(':')
            && !path.is_empty()
            && !lines.is_empty()
            && lines
                .chars()
                .all(|character| character.is_ascii_digit() || character == '-')
        {
            let line = |digits: &str| digits.parse::<usize>().ok().filter(|line| *line > 0);
            let (start, end) = lines.split_once('-').unwrap_or((lines, lines));
            return match (line(start), line(end)) {
                (Some(first), Some(last)) if first <= last => {
                    Ok(Self {
                        path: PathBuf::from(path),
                        lines: Some((first, last)),
                    })
                },
                _ => Err(SpanError(text.to_owned())),
            };
        }
        Ok(Self {
            path: PathBuf::from(text),
            lines: None,
        })
    }
}

impl fmt::Display for SpanRequest {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let path = self.path.display();
        match self.lines {
            None => write!(f, "{path}"),
            Some((start, end)) if start == end => write!(f, "{path}:{start}"),
            Some((start, end)) => write!(f, "{path}:{start}-{end}"),
        }
    }
}

/// Kept in the review log as the text `--context` accepts, so a plain path is a whole file.
impl Serialize for SpanRequest {
    #[inline]
    fn serialize<Writer>(&self, serializer: Writer) -> Result<Writer::Ok, Writer::Error>
    where
        Writer: Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for SpanRequest {
    #[inline]
    fn deserialize<Reader>(deserializer: Reader) -> Result<Self, Reader::Error>
    where
        Reader: Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse()
            .map_err(Reader::Error::custom)
    }
}
