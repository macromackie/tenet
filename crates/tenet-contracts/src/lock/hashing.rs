use crate::{document::FrontmatterError, parse::with_name};

/// Hash a `CONTRACT.md` for its entry's `hash`: its exact bytes without the `name` value.
///
/// A locally renamed copy still matches upstream. This is the version 1 meaning, which every lock
/// version keeps, so a stored hash never changes meaning.
#[inline]
pub fn hash_contract(document: &str) -> Result<String, FrontmatterError> {
    Ok(format!(
        "blake3:{}",
        blake3::hash(with_name(document, "")?.as_bytes()).to_hex()
    ))
}

/// Hash a file for an entry's `files`: with CRLF line endings read as LF, so Git's line-ending
/// settings never make an installed asset or skill file look edited.
#[must_use]
#[inline]
pub fn hash_file(bytes: &[u8]) -> String {
    let mut empty = blake3::Hasher::new();
    let hasher = bytes
        .split_inclusive(|byte| *byte == b'\n')
        .fold(&mut empty, |partial, line| {
            match line.strip_suffix(b"\r\n") {
                Some(text) => partial.update(text).update(b"\n"),
                None => partial.update(line),
            }
        });
    format!("blake3:{}", hasher.finalize().to_hex())
}

/// Hash a `SKILL.md` for its entry's `files`, as [`hash_file`] does, without its `name` value.
#[inline]
pub fn hash_skill(document: &str) -> Result<String, FrontmatterError> {
    Ok(hash_file(with_name(document, "")?.as_bytes()))
}
