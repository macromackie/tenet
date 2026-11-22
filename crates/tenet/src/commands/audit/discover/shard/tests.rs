use core::fmt::Write as _;
use std::fs;

use anyhow::Result;

use crate::commands::audit::discover::discover_modules;

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
