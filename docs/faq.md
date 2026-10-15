# FAQ

**Does code leave my infrastructure?** Not unless you enable LLM steps. Parsing, resolution, embeddings,
search and the ledger run locally and write to the directory or bucket you choose. `enrich`, `flows`,
`guide` and the distiller send code or merge-request text to the LLM endpoint you configure; `--no-llm`
turns all of them off.

**Do I need SCIP indexers?** No. tree-sitter resolution covers the supported languages; when a SCIP index
or indexer is present, its answers are used as well.

**Does it read agent session transcripts?** No. The ledger fills from people, from agents through MCP, from
git history and from merged merge requests.

**How fresh is the index?** As fresh as the last build. CI builds a new version on each merge; readers
switch atomically, and hooks on a remote index read a local mirror that refreshes in the background.

**What if a record is wrong?** Retract it (`comind review <id> retract`). Records follow the code: changed
code marks them stale, deleted code orphans them, and agents only see trusted ones.

**Which languages?** Python, TypeScript, JavaScript, Go, Java, Kotlin and Rust.
