# Preflight and checks

Before a change lands: what the team already knows about the code it touches.

```bash
comind preflight .                          # your uncommitted changes
comind preflight . --base origin/main --strict   # in an MR pipeline; fails on violated checks
```

Preflight lists recorded constraints and dead ends on the touched code, contracts other services
consume, the blast radius across repos, files that usually change together but are missing, and
failing constraint checks.

### Constraints that enforce themselves

A constraint can carry a rule that runs against the graph or the manifests:

```bash
comind remember constraint --on raw_query --title "Handlers never call raw SQL" \
  --field "check=forbid calls from svc/app/handlers/** to **::raw_*"
comind remember constraint --on Settings --title "No lance 1.0.1" --field "check=forbid dep lance =1.0.1"
comind ledger --check                       # non-zero exit when a `must` rule is violated
```

Rules: `forbid [<edge kind>] from <glob>[::<name>] to <glob>[::<name>]` and
`forbid dep <package> [<version>]`. A passing rule ranks the constraint as T4. An attempt recorded
with `--field "dep=lance =1.0.1"` is reported as *retry condition met* once no manifest uses that
version any more.
