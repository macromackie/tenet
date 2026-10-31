"""Resolve Rust's explicit file imports without executing macros or build scripts."""

from dependencies.rust.scan import RustSources

__all__ = ["RustSources"]
