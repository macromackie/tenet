"""Resolve explicit Rust source paths using parsed syntax and native Cargo entry files."""

from typing import TYPE_CHECKING

import tree_sitter_rust
from tree_sitter import Language, Parser

from dependencies.constants import CARGO_MANAGER
from dependencies.errors import DependencyRejected
from dependencies.inventory import _walk_files, is_excluded
from dependencies.model import ObservedImport
from dependencies.rust.constants import (
    ASSIGN_OPERATOR,
    ATTRIBUTE_KIND,
    COMMENT_KINDS,
    IDENTIFIER_KIND,
    INCLUDE_MACROS,
    INCLUDE_NAME,
    MACRO_INPUT_KIND,
    MACRO_KIND,
    MACRO_OPERATOR,
    MODULE_KIND,
    MODULE_ROOT_NAMES,
    PATH_ATTRIBUTE,
    RUST_SUFFIX,
    USE_ALIAS_KIND,
)
from dependencies.rust.literals import read_literal

if TYPE_CHECKING:
    from pathlib import Path

    from tree_sitter import Node

    from dependencies.model import ObservedPackage


class RustSources:
    """Own one parsed source inventory and its pending literal inclusions."""

    def __init__(
        self, *, root: Path, packages: tuple[ObservedPackage, ...], exclusions: tuple[str, ...]
    ) -> None:
        """Bind parsed files and pending inclusions to one repository inventory."""
        self.root = root.resolve()
        self.exclusions = exclusions
        self.package_directories = tuple(
            (self.root / package.manifest).parent
            for package in packages
            if package.manager == CARGO_MANAGER
        )
        self.entry_files = frozenset(entry for package in packages for entry in package.entry_files)
        self.parser = Parser(Language(tree_sitter_rust.language()))
        self.imports = list(self._read_entry_imports(packages=packages))
        self.pending_files: list[tuple[Path, bool]] = []
        self.visited_files: set[Path] = set()

    def read(self) -> tuple[ObservedImport, ...]:
        """Read every owned Rust file and any explicitly included fragment once."""
        for file in _walk_files(root=self.root, exclusions=self.exclusions):
            if self._is_owned_source(file=file):
                self.pending_files.append((file, False))
        self.pending_files.extend((self.root / entry, False) for entry in self.entry_files)
        while self.pending_files:
            file, is_fragment = self.pending_files.pop()
            if file not in self.visited_files:
                self.visited_files.add(file)
                self._scan_file(file=file, is_fragment=is_fragment)
        return tuple(
            sorted(
                set(self.imports),
                key=lambda item: (item.source_file, item.target_file, item.specifier),
            )
        )

    def _read_entry_imports(
        self, *, packages: tuple[ObservedPackage, ...]
    ) -> tuple[ObservedImport, ...]:
        return tuple(
            ObservedImport(
                source_file=package.manifest,
                target_file=entry,
                specifier=entry,
                manager=CARGO_MANAGER,
            )
            for package in packages
            for entry in package.entry_files
            if not (self.root / entry).is_relative_to((self.root / package.manifest).parent)
        )

    def _is_owned_source(self, *, file: Path) -> bool:
        relative = file.relative_to(self.root).as_posix()
        if file.suffix != RUST_SUFFIX or is_excluded(path=relative, exclusions=self.exclusions):
            return False
        if not any(file.is_relative_to(directory) for directory in self.package_directories):
            return False
        if file.is_symlink():
            message = f"inspect Rust source {relative!r}: source symlinks are not allowed"
            raise DependencyRejected(message)
        return True

    def _scan_file(self, *, file: Path, is_fragment: bool) -> None:
        tree = self._read_root(file=file, is_fragment=is_fragment)
        module_base = file.parent
        if (
            file.relative_to(self.root).as_posix() not in self.entry_files
            and file.name not in MODULE_ROOT_NAMES
        ):
            module_base = file.with_suffix("")
        self._scan_nodes(node=tree, file=file, path_base=file.parent, module_base=module_base)

    def _read_root(self, *, file: Path, is_fragment: bool) -> Node:
        content = file.read_bytes()
        tree = self.parser.parse(content)
        if tree.root_node.has_error and is_fragment:
            tree = self.parser.parse(b"fn __source_fragment() {\n" + content + b"\n}")
        if tree.root_node.has_error:
            message = f"inspect Rust source {str(file)!r}: syntax could not be resolved"
            raise DependencyRejected(message)
        return tree.root_node

    def _scan_nodes(self, *, node: Node, file: Path, path_base: Path, module_base: Path) -> None:
        attributes: list[Node] = []
        for child in node.named_children:
            if child.type == ATTRIBUTE_KIND:
                attributes.append(child)
            elif child.type not in COMMENT_KINDS:
                self._scan_child(
                    node=child,
                    file=file,
                    bases=(path_base, module_base),
                    attributes=tuple(attributes),
                )
                attributes.clear()
        self._scan_token_includes(node=node, file=file)

    def _scan_child(
        self, *, node: Node, file: Path, bases: tuple[Path, Path], attributes: tuple[Node, ...]
    ) -> None:
        self._inspect_include_alias(node=node)
        if node.type == MODULE_KIND:
            self._scan_module(node=node, file=file, bases=bases, attributes=attributes)
        elif node.type == MACRO_KIND and self._is_include(node=node):
            self._scan_include(node=node, file=file)
        else:
            self._scan_nodes(node=node, file=file, path_base=bases[0], module_base=bases[1])

    def _inspect_include_alias(self, *, node: Node) -> None:
        if node.type != USE_ALIAS_KIND:
            return
        path = self._read_text(node=node.named_children[0])
        if path.rsplit("::", 1)[-1].removeprefix("r#") == INCLUDE_NAME:
            message = "resolve Rust include alias: use a directly named literal include macro"
            raise DependencyRejected(message)

    def _scan_module(
        self, *, node: Node, file: Path, bases: tuple[Path, Path], attributes: tuple[Node, ...]
    ) -> None:
        paths = tuple(
            path for attribute in attributes for path in self._read_attribute_paths(node=attribute)
        )
        name = self._read_module_name(node=node)
        body = node.child_by_field_name("body")
        if body is not None:
            self._scan_inline_module(body=body, file=file, bases=bases, name=name, paths=paths)
        elif paths:
            for path in paths:
                self._record_import(
                    file=file, target=bases[0] / path, specifier=path, is_fragment=False
                )
        else:
            target = self._read_default_module(directory=bases[1], name=name)
            self._record_import(file=file, target=target, specifier=name, is_fragment=False)

    def _read_module_name(self, *, node: Node) -> str:
        name = node.child_by_field_name("name")
        if name is None:
            message = "inspect Rust module: the parser did not retain its name"
            raise DependencyRejected(message)
        return self._read_text(node=name).removeprefix("r#")

    def _scan_inline_module(
        self, *, body: Node, file: Path, bases: tuple[Path, Path], name: str, paths: tuple[str, ...]
    ) -> None:
        directories = tuple(bases[0] / path for path in paths)
        if not directories:
            directories = (bases[1] / name,)
        for directory in directories:
            self._scan_nodes(node=body, file=file, path_base=directory, module_base=directory)

    def _read_attribute_paths(self, *, node: Node) -> tuple[str, ...]:
        paths: list[str] = []
        children = node.children
        for index, child in enumerate(children):
            is_path_name = (
                child.type == IDENTIFIER_KIND
                and self._read_text(node=child).removeprefix("r#") == PATH_ATTRIBUTE
            )
            has_assignment = (
                index + 2 < len(children) and children[index + 1].type == ASSIGN_OPERATOR
            )
            if is_path_name and has_assignment:
                paths.append(read_literal(node=children[index + 2]))
            paths.extend(self._read_attribute_paths(node=child))
        return tuple(paths)

    def _is_include(self, *, node: Node) -> bool:
        macro = node.child_by_field_name("macro")
        return macro is not None and self._read_text(node=macro).replace("r#", "") in INCLUDE_MACROS

    def _scan_include(self, *, node: Node, file: Path) -> None:
        tokens = node.named_children[-1]
        self._record_include_tokens(tokens=tokens, file=file)

    def _scan_token_includes(self, *, node: Node, file: Path) -> None:
        if node.type != MACRO_INPUT_KIND:
            return
        children = node.children
        for index, child in enumerate(children):
            is_include_name = (
                child.type == IDENTIFIER_KIND
                and self._read_text(node=child).removeprefix("r#") == INCLUDE_NAME
            )
            has_invocation = (
                index + 2 < len(children) and children[index + 1].type == MACRO_OPERATOR
            )
            if is_include_name and has_invocation:
                self._record_include_tokens(tokens=children[index + 2], file=file)

    def _record_include_tokens(self, *, tokens: Node, file: Path) -> None:
        values = tokens.named_children
        if len(values) != 1:
            message = f"resolve Rust include in {str(file)!r}: declare a literal source path"
            raise DependencyRejected(message)
        path = read_literal(node=values[0])
        self._record_import(file=file, target=file.parent / path, specifier=path, is_fragment=True)

    def _record_import(
        self, *, file: Path, target: Path, specifier: str, is_fragment: bool
    ) -> None:
        target = target.resolve()
        if not target.is_relative_to(self.root) or not target.is_file():
            message = (
                f"resolve Rust source {specifier!r} in {str(file)!r}: "
                "target is not a repository file"
            )
            raise DependencyRejected(message)
        self.imports.append(
            ObservedImport(
                source_file=file.relative_to(self.root).as_posix(),
                target_file=target.relative_to(self.root).as_posix(),
                specifier=specifier,
                manager="cargo",
            )
        )
        self.pending_files.append((target, is_fragment))

    @staticmethod
    def _read_default_module(*, directory: Path, name: str) -> Path:
        direct = directory / f"{name}.rs"
        legacy = directory / name / "mod.rs"
        if direct.is_file() == legacy.is_file():
            message = (
                f"resolve Rust module {name!r} in {str(directory)!r}: "
                "expected one physical source file"
            )
            raise DependencyRejected(message)
        if direct.is_file():
            return direct
        return legacy

    @staticmethod
    def _read_text(*, node: Node) -> str:
        value = node.text
        if value is None:
            message = "inspect Rust source: parser did not retain syntax bytes"
            raise DependencyRejected(message)
        return value.decode("utf-8")
