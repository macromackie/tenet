use core::error::Error;
use std::{fs, path::Path};

use tenet_contracts::parse;

use super::{profile::RepositoryProfile, surface};

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Unit test asserts recommendation signal matching directly."
)]
fn matches_idempotent_transformations_from_formatter_signals() -> Result<(), Box<dyn Error>> {
    let dir = tempfile::tempdir()?;
    let file = dir.path().join("src/formatter.py");
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(
        &file,
        "import tokenize\n\ndef sort_code(src: str) -> str:\n    return src\n",
    )?;
    let inventory = vec![Path::new("src/formatter.py").to_path_buf()];
    let profile = RepositoryProfile::from_inventory(&inventory);
    let surfaces = surface::inspect(dir.path(), &inventory)?;
    let contract = parse(
        Path::new(".contracts/001-code-idempotent-transformations/CONTRACT.md"),
        "---\nname: code-idempotent-transformations\nmessage: Keep transforms \
         idempotent.\n---\n## Rules\nKeep transforms idempotent.\n",
    )?;
    let matched = surfaces
        .iter()
        .any(|item| item.score(&contract, &profile).is_some());
    assert!(
        matched,
        "expected formatter signals to match code-idempotent-transformations"
    );
    Ok(())
}
