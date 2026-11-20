use alloc::collections::BTreeSet;
use std::path::{Path, PathBuf};

const AGENT_FILES: &[&str] = &["AGENTS.md", "CLAUDE.md"];
const AGENT_TERMS: &[&str] = &["agent", "tool", "orchestrat", "subagent"];
const API_TERMS: &[&str] = &[
    "api", "axum", "actix", "django", "endpoint", "express", "fastapi", "flask", "http", "openapi",
    "route", "router",
];
const DATA_TERMS: &[&str] = &[
    "alembic",
    "database",
    "migration",
    "postgres",
    "query",
    "sql",
    "sqlite",
    "store",
    "table",
    "upload",
];
const EVAL_TERMS: &[&str] = &["benchmark", "eval", "grader", "judge", "rubric"];
const FRONTEND_EXTENSIONS: &[&str] = &["css", "html", "jsx", "scss", "tsx"];
const TERRAFORM_EXTENSIONS: &[&str] = &["tf", "tfvars"];
const LLM_TERMS: &[&str] = &[
    "anthropic",
    "completion",
    "llm",
    "model",
    "openai",
    "openrouter",
    "prompt",
];
const RELEASE_TERMS: &[&str] = &["artifact", "export", "installer", "publish", "release"];
const SECURITY_TERMS: &[&str] = &[
    "auth",
    "credential",
    "oauth",
    "permission",
    "secret",
    "session",
    "signature",
    "token",
    "webhook",
];
const SERVICE_TERMS: &[&str] = &[
    "actix",
    "axum",
    "commit",
    "django",
    "endpoint",
    "express",
    "fastapi",
    "flask",
    "handler",
    "idempotency",
    "job",
    "queue",
    "router",
    "service",
    "transaction",
    "webhook",
    "worker",
];
const TELEMETRY_TERMS: &[&str] = &["opentelemetry", "sentry", "span", "telemetry", "tracing"];

/// Structural repository facts derived from discovered file paths alone.
#[expect(
    clippy::struct_excessive_bools,
    reason = "Each flag represents an independent repository structure fact."
)]
pub(super) struct RepositoryProfile {
    has_agent_instructions: bool,
    has_ci_workflows: bool,
    has_context_notes: bool,
    has_contracts: bool,
    has_docs: bool,
    has_frontend: bool,
    has_mise: bool,
    has_terraform: bool,
    has_uv: bool,
}

impl RepositoryProfile {
    pub(super) fn from_inventory(inventory: &[PathBuf]) -> Self {
        let mut profile = Self {
            has_agent_instructions: false,
            has_ci_workflows: false,
            has_context_notes: false,
            has_contracts: false,
            has_docs: false,
            has_frontend: false,
            has_mise: false,
            has_terraform: false,
            has_uv: false,
        };
        for path in inventory {
            profile.record_path(path);
        }
        profile
    }

    pub(super) fn is_domain_admissible(
        &self,
        contract_name: &str,
        names: &BTreeSet<String>,
        terms: &BTreeSet<String>,
    ) -> bool {
        if names.contains("mise") && !self.has_mise {
            return false;
        }
        if names.contains("uv") && !self.has_uv && !terms.contains("uv") {
            return false;
        }
        if names.contains("terraform") && !self.has_terraform {
            return false;
        }
        let Some((prefix, _)) = contract_name.split_once('-') else {
            return true;
        };
        match prefix {
            "agent" => has_any_term(terms, AGENT_TERMS),
            "api" => has_any_term(terms, API_TERMS),
            "ci" => self.has_ci_workflows,
            "cli" => terms.contains("cli"),
            "context" => {
                if contract_name.contains("instruction") {
                    self.has_agent_instructions
                } else {
                    self.has_context_notes
                }
            },
            "contract" => self.has_contracts,
            "data" => has_any_term(terms, DATA_TERMS),
            "docs" => self.has_docs && (terms.contains("readme") || terms.contains("docs")),
            "eval" => has_any_term(terms, EVAL_TERMS),
            "infra" => self.has_terraform,
            "llm" => has_any_term(terms, LLM_TERMS),
            "release" => has_any_term(terms, RELEASE_TERMS),
            "security" => has_any_term(terms, SECURITY_TERMS),
            "service" => has_any_term(terms, SERVICE_TERMS),
            "telemetry" => has_any_term(terms, TELEMETRY_TERMS),
            "ui" | "web" => self.has_frontend,
            _ => true,
        }
    }

    fn record_path(&mut self, path: &Path) {
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        if AGENT_FILES.contains(&name) || path.starts_with(".agents") {
            self.has_agent_instructions = true;
        }
        if path.starts_with(".github/workflows") {
            self.has_ci_workflows = true;
        }
        if path.starts_with(".context") {
            self.has_context_notes = true;
        }
        if path.starts_with(".contracts") {
            self.has_contracts = true;
        }
        if name == "README.md" || path.starts_with("docs") {
            self.has_docs = true;
        }
        if FRONTEND_EXTENSIONS.contains(&extension)
            || name.starts_with("next.config.")
            || name.starts_with("vite.config.")
        {
            self.has_frontend = true;
        }
        if name == "mise.toml" || path.starts_with("mise-tasks") {
            self.has_mise = true;
        }
        if TERRAFORM_EXTENSIONS.contains(&extension) {
            self.has_terraform = true;
        }
        if name == "uv.lock" {
            self.has_uv = true;
        }
    }
}

fn has_any_term(terms: &BTreeSet<String>, markers: &[&str]) -> bool {
    markers.iter().any(|marker| terms.contains(*marker))
}
