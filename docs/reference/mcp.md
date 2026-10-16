# MCP tools

`comind serve --index-dir <dir>` speaks MCP over stdio. Results are markdown by default, with
structured JSON attached (`--format json` for JSON only).

| Tool | Answers |
|---|---|
| `search` | Graph-aware hybrid search (BM25, vectors, centrality) |
| `find` | Symbols by name or path |
| `zoom` | A symbol's neighbourhood: callers, callees, importers, members, contract roles, known constraints and dead ends |
| `thread` | Forward call trace from a symbol |
| `flow` | A pre-generated walkthrough of an entry point plus its live call trace |
| `repos` | Indexed repositories, their sizes and internal package dependencies |
| `suggest` | Questions the index can answer, optionally about one symbol |
| `ripple` | Blast radius: who depends on this, across repositories and contracts |
| `context_pack` | The minimal read-set to change a symbol safely, within a token budget |
| `preflight` | For planned edits: recorded constraints and dead ends, contracts, blast radius, missing co-changes, failing checks |
| `contracts` | Routes, queues, topics, buckets and tables with their providers and consumers |
| `guide` | Measured conventions and change recipes for a repository, file or symbol |
| `why` | Recorded reasoning about a symbol, with status and trust |
| `remember` | Record a decision, failed attempt, constraint, gotcha, intent or question |
| `review` | Confirm, retract, supersede or dispute a record (machines can only challenge human-backed ones) |
