# Team setup

## One shared index

The index lives in **your** object store (or a local path). Nothing is sent anywhere else.

```bash
comindly link ../shared-lib ../service-a ../service-b --index-dir s3://your-bucket/comindly --no-llm
```

- Run it from CI on every merge to the default branch (see [CI](ci.md)); `--incremental`
  re-parses only changed files. Each run is a new, atomically readable version.
- What gets indexed: symbols and calls/imports across repos (Python, TypeScript/JavaScript, Go,
  Java, Kotlin, Rust; SCIP indexes when present), service contracts (HTTP routes and clients,
  SQS/SNS/S3, SQL tables), Terraform queues/topics/buckets and subscriptions, YAML config values,
  package versions, conventions and co-change recipes.
- `--no-llm` keeps indexing free and local; drop it to add LLM summaries, flow walkthroughs and
  a style guide (sends code to your configured LLM).

Each developer can run `comindly doctor` to check their setup (index, hooks, keys, signing).

## Give agents the index

```bash
comindly serve --index-dir s3://your-bucket/comindly        # MCP server (add to the agent's config)
comindly init-hooks claude                                # Claude Code hooks to paste into settings
```

- **MCP tools**: navigation (`search`, `find`, `zoom`, `thread`, `flow`), impact (`ripple`,
  `context_pack`, `preflight`, `contracts`), conventions (`guide`) and the ledger (`why`,
  `remember`, `review`).
- **Hooks** inject a short digest at session start and the records about a file just before an
  agent edits it (about 60 ms; a remote index is read from a local mirror refreshed in the
  background). Other agents: `comindly hook generic pre-edit` prints plain markdown.

## Team settings

Settings everyone on an index must share live with the index (`<index>/_team`, a versioned LanceDB
table), so every member, CI job and agent runs the same: the LLM endpoint and model, the embedding model
(an index's vectors only work with the model that made them) and which LLM steps run while indexing. They
win over personal settings. Your own `config.toml` keeps what is personal: the index location, AWS
profile, your name and app preferences.

The team settings hold no secrets. Anyone who can read the index can see them; access to the index is
the trust boundary.

Set them once, in the app (Settings → Team) or from the terminal:

```sh
comindly team set name="Platform" llm_model=gpt-6-luna enrich=true
comindly team                    # what is set, and where your API key comes from
```

Saving needs write access to the index; two people saving at once cannot overwrite each other (the
second save is refused and asks to reload).

### API keys

Everyone brings their own LLM API key. comindly looks for it, in order:

1. the entry for the team's name in `~/.config/comindly/credentials`,
2. the default key in that file,
3. `OPENAI_API_KEY`.

The file is created readable only by you. Set it from the app (Settings → Your API key) or the terminal:

```sh
comindly team key              # prompts for the key; stored under the team's name
comindly team key --default    # your default key, for any team
```

```toml
# ~/.config/comindly/credentials
llm_api_key = "sk-..."       # default

[[team]]
name = "Platform"
llm_api_key = "sk-..."
```

CI sets `OPENAI_API_KEY` from its secret store.

### Starting a team

In the app, **Create a team** on the first screen takes a team name and an empty location everyone can
reach: a shared folder, or `s3://bucket/prefix` with your AWS profile. comindly writes the team settings
there (which also checks you can write to it), switches to it and opens **Settings → Repositories** to add
and index the first repositories. It refuses a location that already has a team or an index; join that
one instead.

### Joining

A colleague needs only the index location and their own cloud access:

```sh
aws sso login --profile team-dev                        # their own access to the bucket
comindly join s3://bucket/comindly --profile team-dev      # remembers both
```

or, in the app, **Join a team** on the first screen (index location, AWS profile); this works as soon as
the team exists, before anything is indexed. For LLM steps they add
their own API key as above.
