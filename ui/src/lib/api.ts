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
  provenance: { author: string; actor: string; source: string; agent: string | null };
  created: string;
}

export interface Entry {
  record: LedgerRecord;
  status: string;
  trust: string;
  current: string[];
  related: [string, string][];
  verified: boolean | null;
}

export interface Convention {
  repo: string;
  dimension: string;
  key: string;
  rule: string;
  support: number;
  total: number;
  exemplars: string[];
  counterexamples: string[];
}

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
}

async function get<T>(path: string): Promise<T> {
  const r = await fetch(path);
  if (!r.ok) throw new Error(`${path}: ${r.status}`);
  return r.json();
}

export const api = {
  overview: () => get<Overview>("/api/overview"),
  orgmap: (at?: number) => get<OrgMap>(at === undefined ? "/api/orgmap" : `/api/orgmap?at=${at}`),
  versions: () => get<string[]>("/api/versions"),
  contracts: () => get<ContractView[]>("/api/contracts"),
  ledger: () => get<Entry[]>("/api/ledger"),
  conventions: () => get<Convention[]>("/api/conventions"),
  find: (q: string) => get<Node[]>(`/api/find?q=${encodeURIComponent(q)}`),
  node: (focus: string) => get<NodeDetail>(`/api/node?focus=${encodeURIComponent(focus)}`),
  review: async (id: string, action: "confirm" | "retract", reason = "") => {
    const r = await fetch("/api/review", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ id, action, reason }),
    });
    return (await r.json()) as { ok: boolean; message: string };
  },
};

/** When a record's fact happened: the harvested commit date, else when it was recorded. */
export const when = (e: Entry) => (e.record.fields.at as string | undefined) ?? e.record.created;

export const isChallenged = (e: Entry) =>
  !["superseded", "retracted"].includes(e.status) &&
  e.related.some(([k]) => ["supersedes", "retracts", "disputes"].includes(k));
