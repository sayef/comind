---
title: comind
hide:
  - navigation
  - toc
---

<section class="cm-hero">
  <img class="cm-logo" src="assets/logo.svg#only-light" alt="" width="88">
  <img class="cm-logo" src="assets/logo-dark.svg#only-dark" alt="" width="88">
  <h1>Cross-repo code intelligence for coding agents</h1>
  <p class="cm-lede">comind indexes a team's repositories into one versioned graph in your own storage and
  serves it to agents over MCP: who breaks if this changes, which services call this route, and what the
  team already learned the hard way about this code.</p>
  <div class="cm-actions">
    <a class="md-button md-button--primary" href="getting-started/">Get started</a>
    <a class="md-button" href="https://github.com/sayef/comind">GitHub</a>
  </div>
  <code class="cm-install">curl -LsSf https://raw.githubusercontent.com/sayef/comind/main/scripts/install.sh | sh</code>
</section>

<div class="grid cards" markdown>

-   **Deterministic, cross-repo graph**

    Calls, imports, inheritance and instantiation from tree-sitter (and SCIP where available) for Python,
    TypeScript/JavaScript, Go, Java, Kotlin and Rust, linked across repositories.

-   **Service contracts**

    HTTP routes and clients, queues, topics, buckets and SQL tables, from code, YAML config and Terraform,
    so blast radius crosses service boundaries.

-   **A reasoning ledger**

    Decisions, failed attempts and constraints anchored to symbols. They follow the code, go stale when it
    changes, and reach agents only when trusted.

-   **Your storage, one binary**

    A local directory or your own bucket holds everything. No server to run; CI builds a new version, every
    agent reads it.

</div>
