# Configuration

Persistent, non-secret defaults live in `config.toml` (`comind config path` shows where, `comind config
init` writes one). Precedence, highest first: command-line flag, environment variable, config file,
built-in default.

```toml
index_dir   = "~/.local/share/comind"   # local path or s3://…
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
# aws_profile = "team-dev"                       # S3: AWS profile when AWS_PROFILE is unset
# sso_auto_login = true                          # app: start `aws sso login` when the session expires
# auto_index = true                              # app: re-index when a default branch moves
# author_name = "Ada Lovelace"                  # name on ledger records (default: git user.name)
# record_names = false                          # keep names off new records
```

Environment: `COMIND_INDEX_DIR`, `COMIND_LLM_MODEL`, `COMIND_EMBED_MODEL`, `COMIND_LLM_BASE_URL`,
`COMIND_FORMAT`, `COMIND_SIGNING_KEY`, `COMIND_ALLOWED_SIGNERS`, `COMIND_AUTHOR_NAME`, `COMIND_AGENT`.

Settings everyone on an index must share (LLM, its key, the embedding model) are team settings stored
with the index and win over this file; see [Team setup](team-setup.md#team-settings).

Secrets stay out of the file: `OPENAI_API_KEY` for the LLM in the environment, and S3 access through the
standard AWS chain ([S3 and compatible storage](storage.md)).
