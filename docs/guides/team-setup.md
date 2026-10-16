# Team setup

## One shared index

The index lives in **your** object store (or a local path). Nothing is sent anywhere else.

```bash
comind link ../shared-lib ../service-a ../service-b --index-dir s3://your-bucket/comind --no-llm
```

- Run it from CI on every merge to the default branch (see [CI](ci.md)); `--incremental`
  re-parses only changed files. Each run is a new, atomically readable version.
- What gets indexed: symbols and calls/imports across repos (Python, TypeScript/JavaScript, Go,
  Java, Kotlin, Rust; SCIP indexes when present), service contracts (HTTP routes and clients,
  SQS/SNS/S3, SQL tables), Terraform queues/topics/buckets and subscriptions, YAML config values,
  package versions, conventions and co-change recipes.
- `--no-llm` keeps indexing free and local; drop it to add LLM summaries, flow walkthroughs and
  a style guide (sends code to your configured LLM).

Each developer can run `comind doctor` to check their setup (index, hooks, keys, signing).

## Give agents the index

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
