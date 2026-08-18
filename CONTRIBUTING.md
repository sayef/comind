# Contributing to Comind

Thanks for your interest! Comind is a single-crate Rust project — small, testable modules behind
the `comind` CLI.

## Development setup

You need Rust (pinned via `rust-toolchain.toml`; rustup installs the right version automatically),
plus `protoc` and `cmake`:

```bash
brew install protobuf cmake        # macOS
# sudo apt-get install -y protobuf-compiler cmake   # Debian/Ubuntu

cargo build
cargo test

# Try the deterministic engine on any repo (no network/S3):
cargo run --example index_and_search -- ../some-repo
```

## Before you commit

CI gates on formatting, clippy, and tests. Run them locally:

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
```

Or install the git hooks to run them automatically (both read `.pre-commit-config.yaml`):

```bash
pre-commit install     # the Python tool
prek install           # or the Rust-native runner (no Python needed)
```

## Project layout

One module per pipeline stage (see [`ARCHITECTURE.md`](ARCHITECTURE.md)):
`model` → `parse` → `git` → `resolve` → `index` → `graph` → `embed` / `search` → `llm` → `mcp`,
with `main.rs` as the CLI.

## Adding a language

Parsing is tree-sitter-based and stays a single binary (no per-language external toolchains). To add
a language, in `src/parse.rs`:

1. add its `tree-sitter-*` grammar to `Cargo.toml`,
2. add an extension arm to `detect()` and `tree_sitter_language()`,
3. extract symbols and intra-file edges for the language's definition/import/call node kinds.

## Pull requests

- Keep changes focused — one logical change per PR.
- Make sure `fmt`, `clippy -D warnings`, and `test` pass (CI enforces them).
- Update `README.md` / `ARCHITECTURE.md` when behavior or the module layout changes.
- Match the surrounding style — comments explain *why*, not *what*; keep them brief.

## License

By contributing, you agree that your contributions are licensed under the MIT License
(see [`LICENSE`](LICENSE)).
