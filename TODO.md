# To do

Open items: things to try, measure or set up. Remove an item once it is done. The longer-term plan is in
[docs/design/roadmap.md](docs/design/roadmap.md).

## To try

- **Merge-request distiller in CI.** Tried on skill-graph !669 and !673 (2 good proposals, 1 correct "nothing"); next, the CI job in the pilot repositories.
- **Team pilot** on two or three repositories: shared index on S3 from CI, hooks and MCP for a few
  developers, preflight in their MR pipelines.

## Before the first release

- First release only once the features are proven in the team pilot (secrets are in place; publishing is
  a manual Release-plz run).
- Review the open Dependabot pull requests (lancedb and lance-index stay pinned).
- Check that CI passes once the repository is public, and that `install.sh` resolves the release assets.

## Dependencies

- **lancedb 0.37.1 pin.** 0.38–0.40 need the heavy `remote` feature. When a later version builds without it,
  move up, drop the pin from Dependabot, and remove the quick-xml advisory ignores from `deny.toml`.

## Gaps

- **Large graphs in the web UI.** Canvas rendering is fine for the org map and per-symbol views; a whole-org
  symbol graph would need WebGL.
- **Helm values** as a source of queue, topic and bucket names (no repository to test against yet).
