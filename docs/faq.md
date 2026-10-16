# FAQ

**Does code leave my infrastructure?** Not unless you enable LLM steps. Parsing, resolution, embeddings,
search and the ledger run locally and write to the directory or bucket you choose. `enrich`, `flows`,
and `guide` send code to the LLM endpoint you configure; `--no-llm`
turns all of them off.

**Do I need SCIP indexers?** No. tree-sitter resolution covers the supported languages; when a SCIP index
or indexer is present, its answers are used as well.

**Does it read agent session transcripts?** No. The ledger fills only from people (`comind remember`) and from
agents through MCP and hooks.

**How fresh is the index?** As fresh as the last build. CI builds a new version on each merge; readers
switch atomically. For a remote index, the MCP server, the web UI and hooks read a local copy (under
`~/.cache/comind`) that is synced when they start and refreshed when older than `cache_ttl_secs`
(five minutes); only new files are downloaded. Ledger and convention changes are written straight
to the remote store.

**What if a record is wrong?** Retract it (`comind review <id> retract`). Records follow the code: changed
code marks them stale, deleted code orphans them, and agents only see trusted ones.

**Which languages?** Python, TypeScript, JavaScript, Go, Java, Kotlin and Rust.
