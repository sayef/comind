---
title: comindly
hide:
  - navigation
  - toc
---

<section class="cm-hero">
  <div class="cm-hero__text">
    <p class="cm-eyebrow">MCP · cross-repo · self-hosted</p>
    <h1 class="cm-title" aria-label="Cross-repo code intelligence for coding agents.">
      <span class="cm-rotate" aria-hidden="true">
        <span>Cross-repo code intelligence for coding agents<span class="cm-dot">.</span></span>
        <span>Know who breaks before you change it<span class="cm-dot">.</span></span>
        <span>Every repository, one graph<span class="cm-dot">.</span></span>
        <span>Your bucket is the server<span class="cm-dot">.</span></span>
        <span>What the team learned, where agents look<span class="cm-dot">.</span></span>
      </span>
    </h1>
    <p class="cm-lede">comindly indexes a team's repositories into one versioned graph in your own storage and
    serves it to agents over MCP: who breaks if this changes, which services call this route, and what the
    team already learned the hard way about this code.</p>
    <div class="cm-actions">
      <a class="md-button md-button--primary" href="getting-started/">Get started</a>
      <a class="md-button" href="https://github.com/sayef/comindly">GitHub</a>
    </div>
    <code class="cm-install">curl -LsSf https://raw.githubusercontent.com/sayef/comindly/main/scripts/install.sh | sh</code>
  </div>
  <div class="cm-hero__shot">
    <img src="assets/screenshots/explorer.webp#only-light" alt="comindly showing create_order in an example shop: its callers across repositories, blast radius and flow walkthrough" width="1440" height="900">
    <img src="assets/screenshots/explorer-dark.webp#only-dark" alt="comindly showing create_order in an example shop: its callers across repositories, blast radius and flow walkthrough" width="1440" height="900">
  </div>
</section>

<div class="cm-stats">
  <div><strong>6 languages</strong><span>Python, TypeScript/JS, Go, Java, Kotlin, Rust</span></div>
  <div><strong>1 binary</strong><span>CLI, MCP server, web UI and desktop app</span></div>
  <div><strong>0 servers</strong><span>a folder or your own bucket holds the index</span></div>
  <div><strong>~0.2 s</strong><span>search against an index in S3</span></div>
</div>

<figure class="cm-shot" markdown>
  ![The org map of an example shop: five repositories and the service contracts between them](assets/screenshots/overview.webp#only-light)
  ![The org map of an example shop: five repositories and the service contracts between them](assets/screenshots/overview-dark.webp#only-dark)
  <figcaption>The org map of an example shop: five repositories in three languages, linked by HTTP routes, an SNS topic and an SQS queue.</figcaption>
</figure>

<div class="grid cards" markdown>

-   **Deterministic, cross-repo graph**

    ---

    Calls, imports, inheritance and instantiation from tree-sitter (and SCIP where available) for Python,
    TypeScript/JavaScript, Go, Java, Kotlin and Rust, linked across repositories.

    [:octicons-arrow-right-24: Concepts](concepts.md)

-   **Service contracts**

    ---

    HTTP routes and clients, queues, topics, buckets and SQL tables, from code, YAML config and Terraform,
    so blast radius crosses service boundaries.

    [:octicons-arrow-right-24: Service contracts](guides/contracts.md)

-   **A reasoning ledger**

    ---

    Decisions, failed attempts and constraints anchored to symbols. They follow the code, go stale when it
    changes, and reach agents only when trusted.

    [:octicons-arrow-right-24: The ledger](guides/ledger.md)

-   **For people too**

    ---

    A local web UI with the org map, dependency graphs, contracts, internal packages, conventions and
    ledger review, and a guide to connecting your agent.

    [:octicons-arrow-right-24: Desktop, web and terminal UI](guides/ui.md)

-   **Your storage, one binary**

    ---

    A local directory or your own bucket holds everything. No server to run; CI builds a new version, every
    agent reads it.

    [:octicons-arrow-right-24: Team setup](guides/team-setup.md)

</div>
