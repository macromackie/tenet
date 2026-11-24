use alloc::collections::BTreeSet;
use core::error::Error;
use std::{
    fs,
    path::{Path, PathBuf},
};

use tenet_contracts::{Contract, parse};

use super::{profile::RepositoryProfile, split_words, surface};

const DATA_SQL_CONTRACT: (&str, &str) = (
    "data-sql-store-boundaries",
    "Write SQL text and database client calls only in the database owner's store module.",
);
const UI_TOKENS_CONTRACT: (&str, &str) = (
    "ui-design-tokens",
    "Name shared visual decisions as semantic tokens and expose named variants.",
);

fn write_fixture(root: &Path, files: &[(&str, &str)]) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut inventory = Vec::with_capacity(files.len());
    for (relative, content) in files {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, content)?;
        inventory.push(Path::new(relative).to_path_buf());
    }
    Ok(inventory)
}

fn build_contract(name: &str, message: &str) -> Result<Contract, Box<dyn Error>> {
    let path = Path::new(".contracts")
        .join(format!("001-{name}"))
        .join("CONTRACT.md");
    let document = format!("---\nname: {name}\nmessage: {message}\n---\n## Rules\n{message}\n");
    Ok(parse(&path, &document)?)
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Unit test asserts recommendation signal matching directly."
)]
fn matches_idempotent_transformations_from_formatter_signals() -> Result<(), Box<dyn Error>> {
    let dir = tempfile::tempdir()?;
    let inventory = write_fixture(
        dir.path(),
        &[(
            "src/formatter.py",
            "import tokenize\n\ndef sort_code(src: str) -> str:\n    return src\n",
        )],
    )?;
    let profile = RepositoryProfile::from_inventory(&inventory);
    let surfaces = surface::inspect(dir.path(), &inventory)?;
    let contract = build_contract(
        "code-idempotent-transformations",
        "Keep transforms idempotent.",
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

const LUXON_LIKE_FILES: &[(&str, &str)] = &[
    (
        "src/datetime.js",
        "export class DateTime { toSQL() { return ''; } }\nexport function toSQL(dt) { const \
         table = [dt]; return String(table.length); }\n",
    ),
    (
        "src/impl/tokenParser.js",
        "export function explainToken(token) { return token; }\n",
    ),
    (
        "site/demo/index.html",
        "<!doctype html><html><body><div class=\"demo\">Luxon</div></body></html>\n",
    ),
    (
        "site/demo/style.css",
        ":root { --color-bg: #fff; }\nbody { color: var(--color-bg); }\n",
    ),
];

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Unit test asserts false-positive exclusion on a pure JS library fixture."
)]
fn pure_js_library_avoids_sql_and_ui_false_positives() -> Result<(), Box<dyn Error>> {
    let dir = tempfile::tempdir()?;
    let inventory = write_fixture(dir.path(), LUXON_LIKE_FILES)?;
    let profile = RepositoryProfile::from_inventory(&inventory);
    let surfaces = surface::inspect(dir.path(), &inventory)?;
    assert!(
        surfaces.iter().all(|item| !item.path().starts_with("site")),
        "expected site/demo assets to be excluded from surfaces"
    );
    let sql_contract = build_contract(DATA_SQL_CONTRACT.0, DATA_SQL_CONTRACT.1)?;
    let ui_contract = build_contract(UI_TOKENS_CONTRACT.0, UI_TOKENS_CONTRACT.1)?;
    assert!(
        surfaces
            .iter()
            .all(|item| item.score(&sql_contract, &profile).is_none()),
        "expected pure JS library to avoid data-sql-store-boundaries"
    );
    assert!(
        surfaces
            .iter()
            .all(|item| item.score(&ui_contract, &profile).is_none()),
        "expected pure JS library to avoid ui-design-tokens"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Unit test asserts real frontend app matches ui-design-tokens accurately."
)]
fn real_frontend_app_matches_ui_design_tokens() -> Result<(), Box<dyn Error>> {
    let dir = tempfile::tempdir()?;
    let inventory = write_fixture(
        dir.path(),
        &[
            (
                "src/components/Button.tsx",
                concat!(
                    "import React from 'react';\n",
                    "export function Button({ label }: { label: string }) {\n",
                    "    return <button className=\"btn\">{label}</button>;\n",
                    "}\n",
                ),
            ),
            (
                "src/styles/tokens.css",
                ":root {\n    --color-surface: #ffffff;\n    --color-accent: #2563eb;\n}\n",
            ),
            (
                "src/impl/tokenParser.js",
                "export function parseToken(input) { return input; }\n",
            ),
        ],
    )?;
    let profile = RepositoryProfile::from_inventory(&inventory);
    let surfaces = surface::inspect(dir.path(), &inventory)?;
    let ui_contract = build_contract(UI_TOKENS_CONTRACT.0, UI_TOKENS_CONTRACT.1)?;
    let matched_tokens_css = surfaces.iter().any(|item| {
        item.path() == Path::new("src/styles/tokens.css")
            && item.score(&ui_contract, &profile).is_some()
    });
    let matched_parser = surfaces.iter().any(|item| {
        item.path() == Path::new("src/impl/tokenParser.js")
            && item.score(&ui_contract, &profile).is_some()
    });
    assert!(
        matched_tokens_css,
        "expected src/styles/tokens.css to match ui-design-tokens"
    );
    assert!(
        !matched_parser,
        "expected src/impl/tokenParser.js not to match ui-design-tokens"
    );
    Ok(())
}

fn inspect_fixture(
    root: &Path,
    files: &[(&str, &str)],
) -> Result<(RepositoryProfile, Vec<surface::Surface>), Box<dyn Error>> {
    let inventory = write_fixture(root, files)?;
    let profile = RepositoryProfile::from_inventory(&inventory);
    let surfaces = surface::inspect(root, &inventory)?;
    Ok((profile, surfaces))
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Unit test asserts real SQL store matches data-sql-store-boundaries."
)]
fn real_sql_store_matches_data_sql_store_boundaries() -> Result<(), Box<dyn Error>> {
    let dir = tempfile::tempdir()?;
    let (profile, surfaces) = inspect_fixture(
        dir.path(),
        &[(
            "src/db/orders.ts",
            concat!(
                "import { Pool } from 'pg';\n",
                "export async function listOrders(pool: Pool) {\n",
                "    return pool.query('SELECT id, total FROM orders');\n",
                "}\n",
            ),
        )],
    )?;
    let sql_contract = build_contract(DATA_SQL_CONTRACT.0, DATA_SQL_CONTRACT.1)?;
    let matched = surfaces.iter().any(|item| {
        item.path() == Path::new("src/db/orders.ts")
            && item.score(&sql_contract, &profile).is_some()
    });
    assert!(
        matched,
        "expected src/db/orders.ts to match data-sql-store-boundaries"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Unit test asserts docs paths preserve has_docs without setting has_frontend."
)]
fn docs_directory_sets_docs_without_frontend_or_code_surfaces() -> Result<(), Box<dyn Error>> {
    let dir = tempfile::tempdir()?;
    let inventory = write_fixture(
        dir.path(),
        &[
            ("README.md", "# Library\n"),
            ("docs/index.html", "<html><body>Docs</body></html>\n"),
            (
                "docs/demo.ts",
                "export function demoHelper() { return 1; }\n",
            ),
        ],
    )?;
    let profile = RepositoryProfile::from_inventory(&inventory);
    let surfaces = surface::inspect(dir.path(), &inventory)?;
    assert!(
        surfaces.is_empty(),
        "expected docs/ files to be excluded from code surfaces"
    );
    let ui_terms = BTreeSet::from(["ui".to_owned(), "css".to_owned(), "token".to_owned()]);
    assert!(
        !profile.is_domain_admissible(
            UI_TOKENS_CONTRACT.0,
            &split_words(UI_TOKENS_CONTRACT.0),
            &ui_terms,
            &ui_terms,
        ),
        "expected docs/index.html not to enable ui-* contracts"
    );
    let docs_terms = BTreeSet::from(["readme".to_owned(), "docs".to_owned()]);
    assert!(
        profile.is_domain_admissible(
            "docs-readme-structure",
            &split_words("docs-readme-structure"),
            &docs_terms,
            &BTreeSet::new(),
        ),
        "expected README.md/docs to keep docs-* contracts admissible"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Unit test asserts domain contracts reject accidental single-word overlaps."
)]
fn domain_contracts_reject_single_accidental_word_without_signal() -> Result<(), Box<dyn Error>> {
    let dir = tempfile::tempdir()?;
    let (profile, surfaces) = inspect_fixture(
        dir.path(),
        &[(
            "src/utils/session.ts",
            concat!(
                "import { select } from './pick';\n",
                "export function formatSession(input: string) { return select(input); }\n",
            ),
        )],
    )?;
    let sql_contract = build_contract(DATA_SQL_CONTRACT.0, DATA_SQL_CONTRACT.1)?;
    let security_contract = build_contract(
        "security-browser-sessions",
        "Protect browser session cookies.",
    )?;
    assert!(
        surfaces
            .iter()
            .all(|item| item.score(&sql_contract, &profile).is_none()),
        "expected JS import {{ select }} from ... not to trigger SQL signal"
    );
    assert!(
        surfaces
            .iter()
            .all(|item| item.score(&security_contract, &profile).is_none()),
        "expected single accidental word session without signal not to trigger security contract"
    );
    Ok(())
}
