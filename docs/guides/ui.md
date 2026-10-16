# Terminal and web UI

## Web UI

`comind ui --web [--port 7878]` serves the web UI on 127.0.0.1 only. It is embedded in the binary;
nothing else is needed.

- **Overview**: counts, the org map (repositories and the service contracts between them; click a
  repository to see what a change there can reach), a version slider to compare with earlier index
  versions, symbols per repository and version skew.
- **Contracts**: every route, queue, topic, bucket and table with its providers and consumers, filtered
  by type, by whether both sides are indexed, and by name.
- **Ledger**: records that need review (challenged, stale or orphaned, and unconfirmed machine
  records) with confirm and retract, and the full timeline.
- **Explorer**: one symbol with its neighbourhood graph, blast radius, contracts and recorded reasoning.
- **Conventions**: measured conventions per repository and org-wide; add your own, edit or hide
  measured ones (hidden ones can be restored).

Press <kbd>⌘K</kbd> (<kbd>Ctrl K</kbd>) anywhere to search symbols, files and contracts. The theme follows
the system and can be toggled. Against a read-only index, confirm and retract are hidden.

## Terminal UI

`comind ui` is the compact terminal view: overview, contracts, ledger (with a timeline), explorer and
conventions.
