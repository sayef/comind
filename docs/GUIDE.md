# Comind team guide

How a team runs comind day to day: one shared index in your own store, agents that see it over
MCP and hooks, and a ledger of what the code alone does not say.

## 1. One shared index

The index lives in **your** object store (or a local path). Nothing is sent anywhere else.

```bash
comind link ../shared-lib ../service-a ../service-b --index-dir s3://your-bucket/comind --no-llm
```

- Run it from CI on every merge to the default branch (see [CI.md](CI.md)); `--incremental`
  re-parses only changed files. Each run is a new, atomically readable version.
- What gets indexed: symbols and calls/imports across repos (Python, TypeScript/JavaScript, Go,
  Java, Kotlin, Rust; SCIP indexes when present), service contracts (HTTP routes and clients,
  SQS/SNS/S3, SQL tables), Terraform queues/topics/buckets and subscriptions, YAML config values,
  package versions, conventions and co-change recipes.
- `--no-llm` keeps indexing free and local; drop it to add LLM summaries, flow walkthroughs and
  a style guide (sends code to your configured LLM).

## 2. Give agents the index

```bash
comind serve --index-dir s3://your-bucket/comind        # MCP server (add to the agent's config)
comind init-hooks claude                                # Claude Code hooks to paste into settings
```

- **MCP tools**: navigation (`search`, `find`, `zoom`, `thread`, `flow`), impact (`ripple`,
  `context_pack`, `preflight`, `contracts`), conventions (`guide`) and the ledger (`why`,
  `remember`, `review`).
- **Hooks** inject a short digest at session start and the records about a file just before an
  agent edits it (about 60 ms; a remote index is read from a local mirror refreshed in the
  background). Other agents: `comind hook generic pre-edit` prints plain markdown.

## 3. The ledger: what the code does not say

Records are short, typed facts anchored to symbols: an `attempt` that failed or was reverted, a
`decision` with its rejected alternatives, a `constraint`, a `gotcha`, an `intent`, a `question`.
They follow the code: moved code keeps its records, changed code marks them **stale**, deleted
code marks them **orphaned**.

Where records come from:

| Source | How | Trust |
|---|---|---|
| A person | `comind remember constraint --on get_user --title "…"` | T3 |
| An agent | MCP `remember` (an attempt needs evidence) | T1–T2 |
| Git history | `comind harvest <repos>`: reverts and `Constraint:` / `Rejected:` / `Decision:` / `Gotcha:` / `Intent:` commit trailers | T0–T2 |
| Merged MRs | `comind distill mr` in CI: the LLM proposes up to three records per merged MR with a discussion | T1 |

Hooks and the ledger sections of `zoom`/`context_pack` only use records at **T2 or higher**
(`why` shows everything, with its trust badge). Proposals (T1) wait in the inbox until a person
confirms them:

```bash
comind ledger --inbox                       # stale, orphaned and unconfirmed records
comind review <id> confirm                  # or: retract --reason "…", supersede, dispute
comind ui --web                             # the same, with a timeline, in the browser
```

Ten minutes a week of review is what keeps the ledger worth reading. Agent session transcripts
are never read.

## 4. Before a change

```bash
comind preflight .                          # your uncommitted changes
comind preflight . --base origin/main --strict   # in an MR pipeline; fails on violated checks
```

Preflight lists recorded constraints and dead ends on the touched code, contracts other services
consume, the blast radius across repos, files that usually change together but are missing, and
failing constraint checks.

### Constraints that enforce themselves

A constraint can carry a rule that runs against the graph or the manifests:

```bash
comind remember constraint --on raw_query --title "Handlers never call raw SQL" \
  --field "check=forbid calls from svc/app/handlers/** to **::raw_*"
comind remember constraint --on Settings --title "No lance 1.0.1" --field "check=forbid dep lance =1.0.1"
comind ledger --check                       # non-zero exit when a `must` rule is violated
```

Rules: `forbid [<edge kind>] from <glob>[::<name>] to <glob>[::<name>]` and
`forbid dep <package> [<version>]`. A passing rule ranks the constraint as T4. An attempt recorded
with `--field "dep=lance =1.0.1"` is reported as *retry condition met* once no manifest uses that
version any more.

## 5. Look around

`comind ui --web` (localhost only): org map with a blast-radius lens and a version slider, service
contracts, ledger review and timeline, explorer with a dependency graph per symbol, conventions.
`comind ui` is the compact terminal view.

## 6. Trust and privacy

- **Signing**: with an SSH signing key configured (`signing_key`, or git `user.signingkey` with
  `gpg.format=ssh`), every record is signed; with `allowed_signers` configured, a record that was
  edited or signed by an unknown key drops to T0 and is never shown to agents.
- **People**: records describe code, never people; actors are pseudonymous ids, `@mentions` are
  removed before MR text reaches the LLM.
- **Erasure**: `comind erase <id> | --actor me --yes` deletes records for good, including old
  snapshot versions.
- **Secrets**: records that look like they contain a secret are refused; LLM input is redacted.
- **Poisoning**: records not written by a person are refused when they read like instructions to
  an agent or carry a shell or script payload; a record planted straight into the store with such
  content (or a secret) is treated as T0 and never injected.
