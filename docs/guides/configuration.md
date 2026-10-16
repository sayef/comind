# Configuration

Persistent, non-secret defaults live in `config.toml` (`comind config path` shows where, `comind config
init` writes one). Precedence, highest first: command-line flag, environment variable, config file,
built-in default.

```toml
index_dir   = "~/.local/share/comind"   # local path or s3://…
llm_model   = "gpt-4o-mini"             # any OpenAI-compatible model id
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
# cache_ttl_secs = 300                          # local mirror of a remote index (hooks)
# signing_key = "~/.ssh/id_ed25519.pub"          # sign ledger records (default: git user.signingkey)
# allowed_signers = "~/.config/git/allowed_signers"
```

Environment: `COMIND_INDEX_DIR`, `COMIND_LLM_MODEL`, `COMIND_EMBED_MODEL`, `COMIND_LLM_BASE_URL`,
`COMIND_FORMAT`, `COMIND_SIGNING_KEY`, `COMIND_ALLOWED_SIGNERS`.

Secrets stay in the environment: `OPENAI_API_KEY` for the LLM, the standard AWS variables or an SSO profile
for `s3://` indexes.
