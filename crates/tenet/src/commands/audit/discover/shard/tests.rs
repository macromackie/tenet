use core::fmt::Write as _;
use std::fs;

use anyhow::Result;

use crate::commands::audit::discover::discover_modules;

#[test]
fn discover_modules_excludes_hidden_dirs_from_modules_runtime_and_tests() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    for relative in [
        "pyproject.toml",
        "README.md",
        "src/pkg/core.py",
        "tests/test_core.py",
        ".yarn/plugins/@yarnpkg/plugin.cjs",
        ".yarn/plugins/core.test.js",
        ".venv/lib/site-packages/helper.py",
        ".venv/lib/site-packages/test_core.py",
        ".github/scripts/helper.py",
        ".github/scripts/test_core.py",
    ] {
        let full_path = root.join(relative);
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(full_path, "def core():\n    return 1\n")?;
    }
    let discovered = discover_modules(root, &[], None)?;
    anyhow::ensure!(discovered.modules.len() == 1);
    let only = discovered
        .modules
        .first()
        .ok_or_else(|| anyhow::anyhow!("missing discovered module"))?;
    anyhow::ensure!(only.name == "src/pkg", "unexpected name: {:?}", only.name);
    anyhow::ensure!(only.test_files == vec!["tests/test_core.py"]);
    anyhow::ensure!(only.manifests == vec!["pyproject.toml"]);
    anyhow::ensure!(only.docs == vec!["README.md"]);
    for forbidden in [".yarn/", ".venv/", ".github/"] {
        anyhow::ensure!(
            discovered
                .runtime_paths
                .iter()
                .all(|path| !path.starts_with(forbidden)),
            "hidden directory leaked into runtime_paths: {forbidden:?}"
        );
    }
    Ok(())
}

#[test]
fn discover_modules_windows_monolithic_files_and_deprioritizes_data_tables() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    fs::create_dir_all(root.join("pkg/data"))?;
    fs::create_dir_all(root.join("pkg/_vendored"))?;
    let mut monolith = String::from("import math\n");
    for index in 0_usize..1_600 {
        writeln!(
            monolith,
            "def compute_value_{index}(x):\n    return x + {index} # padding"
        )?;
    }
    fs::write(root.join("pkg/monolith.py"), monolith)?;
    fs::write(root.join("pkg/data/table.py"), "VALUES = (1, 2, 3)\n")?;
    fs::write(
        root.join("pkg/_vendored/helper.py"),
        "def helper():\n    return 1\n",
    )?;
    let discovered = discover_modules(root, &[], None)?;
    let window_count = discovered
        .modules
        .iter()
        .filter(|module| !module.windows.is_empty())
        .count();
    anyhow::ensure!(
        window_count >= 2,
        "expected >= 2 windows, got {window_count:?}"
    );
    anyhow::ensure!(
        discovered
            .modules
            .iter()
            .all(|module| !module.name.contains("_vendored"))
    );
    anyhow::ensure!(
        discovered
            .runtime_paths
            .contains(&String::from("pkg/_vendored/helper.py"))
    );
    let last_name = discovered.modules.last().map(|module| module.name.as_str());
    anyhow::ensure!(
        last_name == Some("pkg/data"),
        "expected data table last, got {last_name:?}"
    );
    Ok(())
}
