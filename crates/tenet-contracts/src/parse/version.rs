use crate::document::DocumentProblem;

/// This Tenet's version, compared with a contract's `requires-tenet`.
const CURRENT: (u64, u64, u64) = (
    parse_decimal(env!("CARGO_PKG_VERSION_MAJOR")),
    parse_decimal(env!("CARGO_PKG_VERSION_MINOR")),
    parse_decimal(env!("CARGO_PKG_VERSION_PATCH")),
);

/// Read a Cargo version part at compile time.
#[expect(
    clippy::panic,
    reason = "only constant evaluation calls this, on the decimal version parts Cargo sets, so a \
              failure stops the build"
)]
const fn parse_decimal(digits: &str) -> u64 {
    match u64::from_str_radix(digits, 10) {
        Ok(number) => number,
        Err(_) => panic!("each Cargo version part is a decimal number"),
    }
}

/// Refuse a `requires-tenet` version newer than this Tenet.
pub(super) fn check_supported(required: &str) -> Result<(), DocumentProblem> {
    let version = parse_version(required).ok_or_else(|| {
        DocumentProblem::RequirementFormat {
            required: required.to_owned(),
        }
    })?;
    if version > CURRENT {
        return Err(DocumentProblem::NewerTenet {
            required: required.to_owned(),
        });
    }
    Ok(())
}

/// Parse a release version `X.Y.Z` written in decimal digits.
#[must_use]
#[inline]
pub fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let mut parts = text.split('.');
    let mut number = || {
        parts
            .next()
            .filter(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))?
            .parse()
            .ok()
    };
    let version = (number()?, number()?, number()?);
    parts.next().is_none().then_some(version)
}
