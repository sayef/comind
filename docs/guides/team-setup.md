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

## Team settings

Settings everyone on an index must share live with the index (`<index>/_team`, a versioned LanceDB
table), so every member, CI job and agent runs the same: the LLM endpoint and model, the LLM API key,
the embedding model (an index's vectors only work with the model that made them) and which LLM steps
run while indexing. They win over personal settings. Your own `config.toml` keeps what is personal:
the index location, AWS profile, your name and app preferences.

Set them once, in the app (Settings → Team) or from the terminal:

```sh
comind team set name="Platform" llm_model=gpt-6-sol enrich=true
comind team set-key            # prompts for the API key; generates the team password (shown once)
comind team                    # what is set, and whether this machine can unlock the key
```

The API key is encrypted with the team password (Argon2id with a random salt, then
XChaCha20-Poly1305). comind generates the password the first time and shows it once: copy it and share it
with your team. The key itself is never shown again. If the key leaks, replace it at the provider and set
the new one with `comind team set-key`.

Saving needs write access to the index; two people saving at once cannot overwrite each other (the
second save is refused and asks to reload).

### Joining

A colleague needs only the index location, their own cloud access and the team password:

```sh
aws sso login --profile team-dev                        # their own access to the bucket
comind join s3://bucket/comind --profile team-dev      # remembers both, asks for the team password once
```

or, in the app, **Join a team** on the first screen (index location, AWS profile, team password). Each machine keeps the passwords of its teams in
`~/.config/comind/credentials` (team name and password, readable only by you); CI takes the password from
`COMIND_TEAM_PASSWORD`. A team needs a name before its key is set, since that is how the entry is found.
