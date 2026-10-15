# Security policy

## Supported versions

comind is pre-1.0. Security fixes go into the latest release.

## Reporting a vulnerability

Please do not open a public issue. Report vulnerabilities privately through
[GitHub security advisories](https://github.com/sayef/comind/security/advisories/new).

Include what you found, how to reproduce it and what you think the impact is. We will acknowledge the
report within five working days and keep you informed until it is resolved. We credit reporters in the
advisory unless you prefer otherwise.

## Scope

comind reads source code you point it at and the index and ledger in storage you configure (a local
directory or your own bucket). It trusts storage that only authorised writers can modify, but ledger text
is injected into coding agents' context, so records are checked for secrets, invisible characters, agent
instructions and shell payloads, and signed records are verified when `allowed_signers` is configured.
Reports showing a ledger record that reaches an agent despite these checks, code execution from indexed
repositories, or the local web UI being reachable from another origin are in scope.
