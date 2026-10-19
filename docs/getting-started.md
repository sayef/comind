# Getting started

comind builds a map of your repositories: what calls what, across repositories and through HTTP
routes, queues, topics, buckets and tables, plus what your team recorded about the code. Your coding
agent reads that map through MCP; you read it in the terminal and a local web UI.

This page takes you from nothing to an agent that uses comind, in about ten minutes.

## 1. Install

Prebuilt binaries for macOS (arm64, x64) and Linux (x64, arm64) are attached to each
[GitHub release](https://github.com/sayef/comind/releases):

```sh
curl -LsSf https://raw.githubusercontent.com/sayef/comind/main/scripts/install.sh | sh
comind --version
```

From source, with Rust 1.95 or newer plus `protoc` and `cmake`:

```sh
git clone https://github.com/sayef/comind && cd comind && cargo build --release
```

## 2. Index your code

The quickest way is the app: `comind app` (macOS) or `comind ui --web --open`, then **Repositories**
→ Scan a folder → tick your repositories → **Index now**. The same from the terminal:

```sh
comind repos add ../shared-lib ../orders ../payments   # remembered in config.toml
comind link --index-dir ~/comind-index                 # each repo's default branch, never your folders
```

Or one repository, as it is on disk, into the default location (`comind config path` shows it):

```sh
comind index .
```

Several repositories that talk to each other belong in one index, so comind can connect them:

```sh
comind link ../shared-lib ../orders ../payments --index-dir ~/comind-index
```

Everything runs locally. With `OPENAI_API_KEY` set (or any OpenAI-compatible endpoint), comind also
writes one-line summaries, flow walkthroughs and a style guide; `--no-llm` turns that off. Re-running
`link` is incremental: only repositories that changed are parsed again.

For a team, build the index in CI into your own bucket (`--index-dir s3://bucket/prefix`) and let
everyone read it; see [Team setup](guides/team-setup.md).

## 3. Look around

```sh
comind search "where do we retry HTTP calls" --index-dir ~/comind-index
comind explore OrderService.place_order --index-dir ~/comind-index   # neighbourhood and blast radius
comind contracts --index-dir ~/comind-index                         # who serves and who calls what
comind app --index-dir ~/comind-index                               # macOS: its own window
comind ui --web --open --index-dir ~/comind-index                   # any OS: in the browser
```

The web UI shows the org map, contracts, the ledger, conventions and internal packages; press
<kbd>⌘K</kbd> to search by name or by describing what you are looking for. Its **Connect** page
repeats the next step with your index path filled in.

## 4. Connect your agent

comind serves MCP over stdio: the agent starts `comind serve` and calls its tools when a question
needs them.

=== "Claude Code"

    ```sh
    claude mcp add --scope user comind -- comind serve --index-dir ~/comind-index
    ```

=== "Cursor"

    `~/.cursor/mcp.json`:

    ```json
    {
      "mcpServers": {
        "comind": { "command": "comind", "args": ["serve", "--index-dir", "~/comind-index"] }
      }
    }
    ```

=== "Codex"

    `~/.codex/config.toml`:

    ```toml
    [mcp_servers.comind]
    command = "comind"
    args = ["serve", "--index-dir", "~/comind-index"]
    ```

=== "VS Code"

    `.vscode/mcp.json`:

    ```json
    {
      "servers": {
        "comind": { "type": "stdio", "command": "comind", "args": ["serve", "--index-dir", "~/comind-index"] }
      }
    }
    ```

For an index in S3, give the server AWS credentials, for example `-e AWS_PROFILE=… -e AWS_REGION=…`
with `claude mcp add`, or an `env` block in the JSON and TOML configs.

Restart the agent and ask it something only a map of the whole codebase can answer:

- "What breaks in other services if I change the signature of `place_order`?"
- "Who calls `GET /orders/{id}`, and which queue does the order service publish to?"
- "What should I read before changing `PaymentClient.charge`?"
- "Why is this function written this way? Did anyone try something else?"

[Using comind with agents](guides/agents.md) lists every tool with the questions it answers.

## 5. Optional: hooks

Hooks put what the team recorded in front of the agent without it asking: records about the
repository when a session starts, and about a file just before the agent edits it.

```sh
comind init-hooks claude     # prints settings to merge into ~/.claude/settings.json
```

`comind init-hooks cursor`, `gemini`, `codex` and `generic` print the equivalent for other agents.

## 6. Check your setup

```sh
comind doctor --index-dir ~/comind-index
```

It checks the index, the ledger, cloud credentials, hooks, the LLM key, signing and SCIP indexers,
and says how to fix what is missing. The web UI's Connect page shows the same checks.

## Next

- [Using comind with agents](guides/agents.md): the tools, hooks and good habits.
- [Team setup](guides/team-setup.md): one shared index from CI.
- [The ledger](guides/ledger.md): recording why code is the way it is.
- [Preflight](guides/preflight.md): checks before a commit or in MR pipelines.
