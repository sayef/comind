# Getting started

## Install

Prebuilt binaries (macOS arm64/x64, Linux x64/arm64) are attached to each
[GitHub release](https://github.com/sayef/comind/releases):

```sh
curl -LsSf https://raw.githubusercontent.com/sayef/comind/main/scripts/install.sh | sh
```

From source, with Rust 1.94.1 or newer plus `protoc` and `cmake`:

```sh
cargo install comind              # once published, or:
git clone https://github.com/sayef/comind && cd comind && cargo build --release
```

## Index one repository

With no `--index-dir`, comind uses a default location (`comind config path` shows it):

```sh
comind index .                                   # graph, contracts, embeddings
comind search "how do we connect to postgres"
comind explore Settings                          # neighbourhood, blast radius, context pack
```

`--no-llm` keeps everything local and free; with `OPENAI_API_KEY` set (or any OpenAI-compatible endpoint),
comind also writes summaries, flow walkthroughs and a style guide.

## Index several repositories together

Cross-repo edges and contracts need the repositories in one index:

```sh
comind link ../shared-lib ../service-a ../service-b --index-dir ./comind-index
comind contracts --index-dir ./comind-index        # who serves and who calls each route, queue, table
comind ui --web --index-dir ./comind-index         # org map, contracts, explorer
```

For a team, point `--index-dir` at your own bucket (`s3://…`) and build it from CI; see
[Team setup](guides/team-setup.md).

## Connect an agent

```json
{
  "mcpServers": {
    "comind": { "command": "comind", "args": ["serve", "--index-dir", "./comind-index"] }
  }
}
```

The [MCP tools](reference/mcp.md) answer navigation, impact and ledger questions. For Claude Code,
`comind init-hooks claude` prints hooks that add the relevant ledger records just before an edit.

## Check your setup

```sh
comind doctor --index-dir ./comind-index
```
