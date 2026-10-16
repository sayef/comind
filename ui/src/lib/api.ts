export interface Node {
  id: string;
  name: string;
  kind: string;
  repo: string;
  location: string;
  signature: string | null;
}

export interface ContractView {
  contract: Node;
  providers: Node[];
  consumers: Node[];
}

export interface Overview {
  repos: { repo: string; symbols: number }[];
  symbols: number;
  edges: number;
  cross_repo_edges: number;
  contracts: number;
  linked_contracts: number;
  ledger: Record<string, number>;
  skew: { package: string; publisher: string; published: string | null; consumer: string; locked: string | null }[];
  writable: boolean;
}

export interface OrgMap {
  nodes: { id: string; name: string; repo: string }[];
  edges: { src: string; dst: string; kind: "code" | "contract"; weight: number }[];
}

export interface Anchor {
  symbol: string;
  repo: string;
  path: string;
  name: string;
  line: number;
}

export interface LedgerRecord {
  id: string;
  kind: string;
  title: string;
  body: string;
  trigger: string | null;
  fields: Record<string, unknown>;
  anchors: Anchor[];
  evidence: string[];
  provenance: { author: string; actor: string; source: string; agent: string | null; name?: string };
  created: string;
}

export interface Entry {
  record: LedgerRecord;
  status: string;
  trust: string;
  current: string[];
  related: [string, string][];
  verified: boolean | null;
  by: string;
  history: LedgerEvent[];
  retired: "outdated" | "wrong" | null;
}

export interface LedgerEvent {
  id: string;
  action: string;
  who: string;
  at: string;
  reason: string;
  counted: boolean;
}

export type ReviewAction = "confirm" | "archive" | "invalidate";

export interface Convention {
  id: string;
  repo: string;
  dimension: string;
  key: string;
  rule: string;
  support: number;
  total: number;
  exemplars: string[];
  counterexamples: string[];
  origin: "measured" | "edited" | "manual";
}

/** A measured convention a person hid. */
export interface HiddenConvention {
  id: string;
  repo: string;
  dimension: string;
  rule: string;
}

export type ConventionChange =
  | { action: "add"; repo: string; dimension: string; rule: string }
  | { action: "edit"; id: string; rule: string }
  | { action: "hide" | "restore"; id: string };

export interface Zoom {
  focus: Node | null;
  container: Node | null;
  members: Node[];
  callers: Node[];
  callees: Node[];
  importers: Node[];
  contracts: { role: string; contract: Node; counterparts: Node[] }[];
  providers: Node[];
  consumers: Node[];
}

export interface NodeDetail {
  zoom: Zoom;
  graph: { nodes: Node[]; edges: { src: string; dst: string; kind: string }[]; focus: string };
  ripple: { total: number; repos: Record<string, number> };
  ledger: Entry[];
  summary: string | null;
  flow: { narration: string; questions: string[] } | null;
}

export interface SearchHit {
  id: string;
  name: string;
  kind: string;
  repo: string;
  location: string;
  signature: string | null;
  summary: string | null;
  dependents: number;
}

export interface Package {
  ecosystem: string;
  name: string;
  publisher: string;
  published: string | null;
  consumer: string;
  requirement: string | null;
  locked: string | null;
  skewed: boolean;
}

export interface Setup {
  index: string;
  remote: boolean;
  version: string;
  checks: { ok: boolean; optional: boolean; message: string }[];
  tools: { name: string; description: string }[];
}

async function get<T>(path: string): Promise<T> {
  const r = await fetch(path);
  if (!r.ok) {
    const body = await r.json().catch(() => null);
    throw new Error(body?.error ?? `${path}: ${r.status}`);
  }
  return r.json();
}

export const api = {
  overview: () => get<Overview>("/api/overview"),
  orgmap: (at?: number) => get<OrgMap>(at === undefined ? "/api/orgmap" : `/api/orgmap?at=${at}`),
  versions: () => get<string[]>("/api/versions"),
  contracts: () => get<ContractView[]>("/api/contracts"),
  ledger: () => get<Entry[]>("/api/ledger"),
  conventions: () => get<{ conventions: Convention[]; hidden: HiddenConvention[] }>("/api/conventions"),
  changeConvention: async (c: ConventionChange) => {
    const r = await fetch("/api/conventions", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(c),
    });
    const out = (await r.json()) as { ok: boolean; message: string };
    if (!out.ok) throw new Error(out.message);
    return out;
  },
  find: (q: string) => get<Node[]>(`/api/find?q=${encodeURIComponent(q)}`),
  search: (q: string) => get<SearchHit[]>(`/api/search?q=${encodeURIComponent(q)}`),
  packages: () => get<Package[]>("/api/packages"),
  setup: () => get<Setup>("/api/setup"),
  context: async (focus: string) => {
    const r = await fetch(`/api/context?focus=${encodeURIComponent(focus)}`);
    return r.text();
  },
  node: (focus: string) => get<NodeDetail>(`/api/node?focus=${encodeURIComponent(focus)}`),
  erase: async (id: string) => {
    const r = await fetch("/api/erase", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ id }),
    });
    return (await r.json()) as { ok: boolean; message: string };
  },
  review: async (id: string, action: ReviewAction, reason = "") => {
    const r = await fetch("/api/review", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ id, action, reason }),
    });
    return (await r.json()) as { ok: boolean; message: string };
  },
};

/** When a record's fact happened (older harvested records carry `at`), else when it was recorded. */
export const when = (e: Entry) => (e.record.fields.at as string | undefined) ?? e.record.created;

const TRUST: Record<string, number> = { T0: 0, T1: 1, T2: 2, T3: 3, T4: 4 };

/** 0 challenged, 1 stale or orphaned, 2 unconfirmed machine record, 3 nothing to do. */
export const attention = (e: Entry) =>
  isChallenged(e) ? 0 : ["stale", "orphaned"].includes(e.status) ? 1 : TRUST[e.trust] < 3 && ["active", "moved"].includes(e.status) ? 2 : 3;

export const isChallenged = (e: Entry) =>
  !["superseded", "retracted"].includes(e.status) &&
  e.related.some(([k]) => ["supersedes", "retracts", "disputes"].includes(k));
