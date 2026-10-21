# Self-hosting

comindly runs as a server for a team from one container image: members sign in by email or with
codes, admins invite people and manage the tokens it uses, and the index lives in your
[storage](storage.md). Agents connect to it over HTTP. Docker, Kubernetes, ECS, Cloud Run or a VM
all work the same way, with the image `ghcr.io/sayef/comindly` (amd64 and arm64, a tag per release).

## Settings

The server reads everything from its environment. Each secret can instead be a file named by the
same variable with `_FILE` (Docker and Kubernetes secrets, Vault, AWS Secrets Manager or Google
Secret Manager mounts), so rotating one is a change in your secret store and a restart.

| Variable | Required | Purpose |
| --- | --- | --- |
| `COMINDLY_PUBLIC_URL` | yes | The address members open, e.g. `https://comindly.example.com`. Setting it turns server mode on. |
| `COMINDLY_ADMIN_EMAILS` | yes | Comma-separated admins. Always admins; they cannot be removed in the app. |
| `COMINDLY_ADMIN_TOKEN` | recommended | Your own key: API access as the first admin, and sign-in when email is unavailable. |
| `COMINDLY_SECRET_KEY` | for stored tokens | Encrypts tokens and keys saved in the app. Without it, they can only come from the environment. |
| `COMINDLY_SECRET_KEY_PREVIOUS` | when rotating | The old key: on start, stored secrets are re-encrypted with the new one. |
| `COMINDLY_SMTP_URL` | for email sign-in | Mail server, e.g. `smtps://user:password@smtp.example.com`. Without it, members sign in with codes. |
| `COMINDLY_SIGN_IN` | optional | `email` or `code`; by default email when `COMINDLY_SMTP_URL` is set, else code. |
| `COMINDLY_SMTP_FROM` | with SMTP | Sender, e.g. `comindly <comindly@example.com>`. |
| `COMINDLY_INDEX_DIR` | yes | Where the index lives (see [Storage](storage.md)). |
| `GH_TOKEN`, `GITLAB_TOKEN` | optional | Repository access, if you keep it with the deployment rather than in the app. |
| `OPENAI_API_KEY` | optional | LLM key, likewise; `COMINDLY_LLM_BASE_URL` for any OpenAI-compatible endpoint. |
| `COMINDLY_TRUST_PROXY` | behind a proxy | `true` when a proxy you run sets `X-Forwarded-For`, so sign-in limits apply per client. |

Everything else (members, sessions, personal tokens, stored secrets) is in a SQLite file under
`/data`. Keep that volume, and back it up with the index.

## Signing in

A server signs everyone in one way: by email, or by codes. The deployment decides.

**By email** (with `COMINDLY_SMTP_URL`): invitations are emailed links for one address that work once
within 7 days; to sign in, members ask for a link that works once, for 15 minutes. Opening a link asks
for a click, so mail scanners cannot use it up.

**By code**: an admin invites an address and hands over the invitation code it gets (once, for that
address, within 7 days). With it, the person signs in for the first time and receives their own
sign-in code, shown once, which they use with their email address from then on. They can make a new
one under Settings → Your access; an admin can reset it for someone who lost theirs. Each new code
signs the person out everywhere else. Codes are generated, never chosen; five wrong ones lock that
address for 15 minutes from the same client, and fifty from anywhere lock it for everyone.

**In both**: admins named in `COMINDLY_ADMIN_EMAILS` can always sign in with the admin token too;
sessions last 30 days; removing a member ends their sessions, codes and tokens.

## Agents

Agents reach the server's tools at `https://<your server>/mcp` (MCP over HTTP) with a personal
access token from Settings → Your access, which also shows the command or config for Claude Code,
Cursor, Codex and VS Code. For Claude Code:

```sh
claude mcp add --transport http --scope user comindly https://comindly.example.com/mcp \
  --header "Authorization: Bearer <token>"
```

Nothing is installed on the developer's machine, and no storage credentials leave the server.
Records an agent writes say which member it works for; after each re-index, agents get the new
index without reconnecting.

Admins manage repositories, indexing, members and the server's tokens; members use everything
else. Ledger records written in the app carry the member's address.

## Tokens and keys

Settings → Integrations shows each token the server uses and where it comes from, never its value.
One the deployment provides wins and is read-only there; otherwise an admin pastes one, and it is
stored encrypted with `COMINDLY_SECRET_KEY`. Read-only tokens are enough: a GitLab group access token
with `read_repository` and `read_api`, or a fine-grained GitHub token with Contents and Metadata read.

## Trying it locally

```sh
scripts/docker-local.sh      # builds the Linux binary here, packages comindly:local, makes .secrets/
docker compose up
```

Open <http://localhost:7878>, sign in as `admin@example.com` and read the link at
<http://localhost:8025> (Mailpit). `COMINDLY_SIGN_IN=code docker compose up -d` switches to codes;
the admin token for the first sign-in is in `.secrets/admin_token`. The index goes to a local S3-compatible store (SeaweedFS).
`docker compose down -v` removes everything.
