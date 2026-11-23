use std::{fs, path::Path};

use anyhow::Result;

use super::find_related_files;

fn write_fixture_file(root: &Path, relative: &str, content: &str) -> Result<()> {
    let full = root.join(relative);
    if let Some(parent) = full.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(full, content)?;
    Ok(())
}

#[test]
fn find_related_files_attaches_rust_crate_and_subcrate_lib_entrypoints() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    write_fixture_file(root, "src/lib.rs", "pub mod lexical;\n")?;
    write_fixture_file(root, "src/lexical/float.rs", "pub fn parse() {}\n")?;
    write_fixture_file(root, "crates/sub/src/lib.rs", "pub mod lexical;\n")?;
    write_fixture_file(
        root,
        "crates/sub/src/lexical/float.rs",
        "pub fn parse() {}\n",
    )?;
    let runtime_paths = vec![
        String::from("src/lib.rs"),
        String::from("src/lexical/float.rs"),
        String::from("crates/sub/src/lib.rs"),
        String::from("crates/sub/src/lexical/float.rs"),
    ];

    let root_related = find_related_files(
        root,
        &[String::from("src/lexical/float.rs")],
        &runtime_paths,
    );
    anyhow::ensure!(
        root_related == vec!["src/lib.rs"],
        "expected src/lib.rs for root leaf shard, got {root_related:?}"
    );

    let subcrate_related = find_related_files(
        root,
        &[String::from("crates/sub/src/lexical/float.rs")],
        &runtime_paths,
    );
    anyhow::ensure!(
        subcrate_related.first().map(String::as_str) == Some("crates/sub/src/lib.rs"),
        "expected nearest workspace subcrate entrypoint first, got {subcrate_related:?}"
    );
    Ok(())
}

#[test]
fn find_related_files_attaches_js_package_name_and_manifest_entrypoints() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    write_fixture_file(
        root,
        "package.json",
        "{\"name\":\"svgo\",\"main\":\"./dist/svgo-node.cjs\"}\n",
    )?;
    write_fixture_file(root, "lib/svgo.js", "export function optimize() {}\n")?;
    write_fixture_file(
        root,
        "plugins/inlineStyles.js",
        "export const fn = () => ({});\n",
    )?;
    let runtime_paths = vec![
        String::from("package.json"),
        String::from("lib/svgo.js"),
        String::from("plugins/inlineStyles.js"),
    ];

    let related = find_related_files(
        root,
        &[String::from("plugins/inlineStyles.js")],
        &runtime_paths,
    );
    anyhow::ensure!(
        related == vec!["lib/svgo.js"],
        "expected lib/svgo.js for JS plugin shard, got {related:?}"
    );
    Ok(())
}

#[test]
fn find_related_files_attaches_ts_standard_entrypoint() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    write_fixture_file(root, "src/index.ts", "export * from './sub/util';\n")?;
    write_fixture_file(root, "src/sub/util.ts", "export const run = () => 1;\n")?;
    let runtime_paths = vec![
        String::from("src/index.ts"),
        String::from("src/sub/util.ts"),
    ];

    let related = find_related_files(root, &[String::from("src/sub/util.ts")], &runtime_paths);
    anyhow::ensure!(
        related == vec!["src/index.ts"],
        "expected src/index.ts for TS leaf shard, got {related:?}"
    );
    Ok(())
}

#[test]
fn find_related_files_attaches_python_ancestor_init_entrypoints() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    write_fixture_file(root, "mypkg/__init__.py", "from .sub.core import run\n")?;
    write_fixture_file(root, "mypkg/sub/__init__.py", "from .core import run\n")?;
    write_fixture_file(root, "mypkg/sub/core.py", "def run():\n    return 1\n")?;
    let runtime_paths = vec![
        String::from("mypkg/__init__.py"),
        String::from("mypkg/sub/__init__.py"),
        String::from("mypkg/sub/core.py"),
    ];

    let related = find_related_files(root, &[String::from("mypkg/sub/core.py")], &runtime_paths);
    anyhow::ensure!(
        related == vec!["mypkg/__init__.py", "mypkg/sub/__init__.py"],
        "expected Python package entrypoints, got {related:?}"
    );
    Ok(())
}

#[test]
fn find_related_files_attaches_go_root_entrypoint_from_go_mod() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    write_fixture_file(root, "go.mod", "module example.com/gin\n\ngo 1.22\n")?;
    write_fixture_file(root, "gin.go", "package gin\nfunc New() {}\n")?;
    write_fixture_file(root, "binding/json.go", "package binding\nfunc Bind() {}\n")?;
    let runtime_paths = vec![
        String::from("go.mod"),
        String::from("gin.go"),
        String::from("binding/json.go"),
    ];

    let related = find_related_files(root, &[String::from("binding/json.go")], &runtime_paths);
    anyhow::ensure!(
        related == vec!["gin.go"],
        "expected gin.go for Go subpackage shard, got {related:?}"
    );
    Ok(())
}

#[test]
fn find_related_files_prepends_entrypoints_deduplicates_and_caps_at_five() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    write_fixture_file(
        root,
        "package.json",
        "{\"name\":\"@scope/demo\",\"main\":\"./src/entry.ts\",\"module\":\"./src/entry.ts\"}\n",
    )?;
    write_fixture_file(root, "src/entry.ts", "export * from './index';\n")?;
    write_fixture_file(root, "src/index.ts", "export const root = true;\n")?;
    write_fixture_file(
        root,
        "src/sub/leaf.ts",
        "import '../entry';\nimport './alpha';\nimport './beta';\nimport './gamma';\nimport \
         './delta';\nexport const leaf = 1;\n",
    )?;
    let mut runtime_paths = vec![
        String::from("package.json"),
        String::from("src/entry.ts"),
        String::from("src/index.ts"),
        String::from("src/sub/leaf.ts"),
    ];
    for helper in ["alpha", "beta", "gamma", "delta"] {
        let rel = format!("src/sub/{helper}.ts");
        write_fixture_file(root, &rel, "export const value = 1;\n")?;
        runtime_paths.push(rel);
    }

    let related = find_related_files(root, &[String::from("src/sub/leaf.ts")], &runtime_paths);
    anyhow::ensure!(
        related.len() == 5,
        "expected cap of 5 related files, got {related:?}"
    );
    anyhow::ensure!(
        related.get(..2) == Some(&[String::from("src/entry.ts"), String::from("src/index.ts")]),
        "expected 2 deduplicated entrypoints prepended first, got {related:?}"
    );
    Ok(())
}
