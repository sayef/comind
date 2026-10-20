# Desktop app, web and terminal UI

## Desktop app (macOS)

`comindly app [--index-dir …]` opens the UI in its own window, like the Ollama app: the operating
system's WebKit, no browser and no bundled Chromium. The window shows a loading screen while the
index loads, then the UI from a private server on a random local port that only this window can
call. Links to other sites open in your browser.

Started from a terminal, it sees your shell's environment (AWS credentials for an S3 index). Linux
and Windows: use the web UI.

## Web UI

`comindly ui --web [--port 7878] [--open]` serves the same UI on 127.0.0.1 only. It prints a link
with an access token; open it once (or pass `--open`) and the browser keeps the token in a cookie,
so the plain address works as a bookmark afterwards. Other programs and web pages cannot call the
UI's API without it. The token is kept in `~/.config/comindly/ui-token`; delete the file to issue a
new one.

- **Overview**: counts, the org map (repositories and the service contracts between them; click a
  repository to see what a change there can reach), a version slider to compare with earlier index
  versions, symbols per repository and version skew.
- **Contracts**: every route, queue, topic, bucket and table with its providers and consumers, filtered
  by type, by whether both sides are indexed, and by name.
- **Ledger**: records that need review (challenged, stale or orphaned, and unconfirmed machine
  records) and the full timeline. **Add record** writes a new one under your name; each record shows
  who wrote it and its review history; confirm, archive, invalidate or delete it. The symbol view
  has **Add to the ledger** with that symbol filled in.
- **Conventions**: measured conventions per repository and org-wide; add your own, edit or hide
  measured ones (hidden ones can be restored). `/conventions?repo=<name>` opens one repository.
- **Packages**: internal packages, who publishes each, and the version every consumer pins, marked
  when it is behind the publisher (or ahead of the version in its source).
- **Settings → Repositories**: what is in the index, and where each repository lives on this machine.
  **From GitHub or GitLab** lists the repositories of an organisation or group (github.com, GitHub
  Enterprise, gitlab.com or a self-hosted GitLab, subgroups included) with your existing git login, a
  `GITHUB_TOKEN` / `GITLAB_TOKEN`, or a token it asks for once (kept by your git credential helper). Tick
  the ones you want: comindly keeps its own copies under `~/.local/share/comindly/repos` (cloned on the
  first **Index now**, without file contents until needed), so you choose no folder. With **Share with the
  team**, the list is stored in the team settings, and a colleague sees "The team has N repositories you
  don't have here" with **Add them**. You can also add a folder, scan a folder, or clone a single
  repository by URL. Every repository is always indexed at its remote's current default branch, whatever
  it is called (main, master, trunk…), asked from the remote on each run. The index records each
  repository's remote URL, so one indexed elsewhere shows **No folder here** with **Locate** and **Clone**.
  comindly indexes all repositories together so cross-repo links stay complete, so **Index now** waits
  until every indexed repository has a folder here, unless you choose to drop the missing ones. With
  **Re-index automatically** on (`auto_index`), the app checks every freshness interval whether a default
  branch moved on the remote and re-indexes without LLM steps when one did.
- **Settings**: the index location (with a connection test), the team settings (shared LLM and
  embedding model), your own API key, the AWS profile for S3, your name on ledger records, and how often to check for a
  new index version. Changing the index location switches the UI without a restart.
- **Get started**: checks of this setup (as `comindly doctor`), how to register the MCP server with
  Claude Code, Cursor, Codex or VS Code with this index's path filled in, hooks, and every MCP tool
  with a question that uses it.
- **Symbol view**, opened from search, a link or a graph node: the dependency graph (dependents
  on the left, dependencies on the right), blast radius, the LLM summary and flow walkthrough when
  the index has them, contracts and recorded reasoning. **Copy context for an agent** copies what to
  read before changing it, with the team's records, as markdown.

### What it looks like

The example company in `examples/acme`, indexed with comindly (`scripts/screenshots.sh`).

![Overview: the org map with the service contracts between repositories](../assets/screenshots/overview.webp#only-light)
![Overview: the org map with the service contracts between repositories](../assets/screenshots/overview-dark.webp#only-dark)
/// caption
Overview: the org map with the service contracts between repositories
///

![A symbol: its dependency graph, blast radius, summary and flow walkthrough](../assets/screenshots/explorer.webp#only-light)
![A symbol: its dependency graph, blast radius, summary and flow walkthrough](../assets/screenshots/explorer-dark.webp#only-dark)
/// caption
A symbol: its dependency graph, blast radius, summary and flow walkthrough
///

![Contracts: routes, topics, queues, buckets and tables with providers and consumers](../assets/screenshots/contracts.webp#only-light)
![Contracts: routes, topics, queues, buckets and tables with providers and consumers](../assets/screenshots/contracts-dark.webp#only-dark)
/// caption
Contracts: routes, topics, queues, buckets and tables with providers and consumers
///

![Ledger: constraints, failed attempts and decisions, with who wrote and confirmed them](../assets/screenshots/ledger.webp#only-light)
![Ledger: constraints, failed attempts and decisions, with who wrote and confirmed them](../assets/screenshots/ledger-dark.webp#only-dark)
/// caption
Ledger: constraints, failed attempts and decisions, with who wrote and confirmed them
///

![Packages: an internal package and a consumer pinned behind its publisher](../assets/screenshots/packages.webp#only-light)
![Packages: an internal package and a consumer pinned behind its publisher](../assets/screenshots/packages-dark.webp#only-dark)
/// caption
Packages: an internal package and a consumer pinned behind its publisher
///

![Search: describe what you are looking for](../assets/screenshots/palette.webp#only-light)
![Search: describe what you are looking for](../assets/screenshots/palette-dark.webp#only-dark)
/// caption
Search: describe what you are looking for
///

![Connect: register the MCP server with your agent](../assets/screenshots/setup.webp#only-light)
![Connect: register the MCP server with your agent](../assets/screenshots/setup-dark.webp#only-dark)
/// caption
Connect: register the MCP server with your agent
///

What is running shows in the header's corner (indexing with its current step, waiting for the AWS sign-in,
or any action in progress), with **Cancel** where it can be stopped; indexing and the sign-in stop with
every process they started. Finished actions appear as toasts in the bottom-right corner.

Press <kbd>⌘K</kbd> (<kbd>Ctrl K</kbd>) anywhere to search: by name, and by describing what you are
looking for ("where do we retry HTTP calls"), which uses the same hybrid search as the MCP `search`
tool. Graphs scroll with the page; pinch or <kbd>⌘</kbd>/<kbd>Ctrl</kbd> and scroll to zoom, drag to
pan. The theme follows the system and can be toggled. Against a read-only index, editing is hidden.

## Terminal UI

`comindly ui` is the compact terminal view: overview, contracts, ledger (with a timeline), explorer and
conventions.
