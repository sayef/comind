# Configuration

Persistent, non-secret defaults live in `config.toml` (`comindly config path` shows where, `comindly config
init` writes one). Precedence, highest first: command-line flag, environment variable, config file,
built-in default.

```toml
index_dir   = "~/.local/share/comindly"   # folder, s3://, gs:// or az://
llm_model   = "gpt-6-luna"             # any OpenAI-compatible model id
embed_model = "minishlab/potion-base-8M"
format      = "md"                      # search: md|table, serve: md|json
embed       = true                      # build steps; --x / --no-x override, --no-llm disables all LLM steps
enrich      = true
flows       = true
guide       = true
# llm_base_url = "http://localhost:11434/v1"   # Ollama, vLLM, LiteLLM, Azure
# max_enrich  = 200
# max_flows   = 50
# scip        = "auto"                          # auto | require | off
# cache_ttl_secs = 300                          # how often serve/ui check for a new index; hooks' mirror
# signing_key = "~/.ssh/id_ed25519.pub"          # sign ledger records (default: git user.signingkey)
# allowed_signers = "~/.config/git/allowed_signers"
# auto_index = true                              # app: re-index when a default branch moves
# author_name = "Ada Lovelace"                  # name on ledger records (default: git user.name)
# record_names = false                          # keep names off new records
```

Environment: `COMINDLY_INDEX_DIR`, `COMINDLY_LLM_MODEL`, `COMINDLY_EMBED_MODEL`, `COMINDLY_LLM_BASE_URL`,
`COMINDLY_FORMAT`, `COMINDLY_SIGNING_KEY`, `COMINDLY_ALLOWED_SIGNERS`, `COMINDLY_AUTHOR_NAME`, `COMINDLY_AGENT`.

Settings everyone on an index must share (the LLM endpoint and model, the embedding model) are team settings stored
with the index and win over this file; see [Team setup](team-setup.md#team-settings).

Secrets stay out of the file: `OPENAI_API_KEY` for the LLM in the environment, and bucket access through
the platform's own credentials ([Storage](storage.md)).
