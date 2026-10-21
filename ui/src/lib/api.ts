export interface Node {
  id: string;
  name: string;
  kind: string;
  repo: string;
  location: string;
  signature: string | null;
}

/** A flow's entry point and how far its flow reaches. */
export interface FlowRow {
  entry: Node;
  steps: number;
  repos: string[];
  contracts: string[];
  summary: string | null;
  narrated: boolean;
}

/** A step of a flow, reached from `from` via `via`. */
export interface FlowStep {
  node: Node;
  depth: number;
  via: string;
  from: string;
}

export interface FlowDetail {
  entry: Node;
  narration: string | null;
  questions: string[] | null;
  steps: FlowStep[];
}

export interface SourceLines {
  path: string;
  line: number;
  lines: { n: number; text: string }[];
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
  root: string | null;
  problem: string | null;
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

export interface Settings {
  config_path: string;
  index_dir: string | null;
  index_effective: string;
  author_name: string | null;
  author_effective: string | null;
  record_names: boolean;
  llm_base_url: string | null;
  llm_model: string;
  llm_key: KeySource;
  cache_ttl_secs: number;
  auto_index: boolean;
  defaults: Defaults;
}

export interface TeamSettings {
  name?: string;
  llm_base_url?: string;
  llm_model?: string;
  embed_model?: string;
  enrich?: boolean;
  flows?: boolean;
  guide?: boolean;
}

/** Where your API key comes from: your credentials file (team entry or default) or OPENAI_API_KEY. */
export type KeySource = "team" | "default" | "environment" | "server" | "none";

export interface Team {
  settings: TeamSettings;
  key_source: KeySource;
  version: number | null;
  changed_by: string | null;
  changed_at: string | null;
  defaults: Defaults;
}

/** comindly's built-in values, used where a setting is empty. */
export interface Defaults {
  llm_model: string;
  llm_base_url: string;
  embed_model: string;
}

export type SettingsChange = Partial<{
  index_dir: string;
  author_name: string;
  record_names: boolean;
  llm_base_url: string;
  llm_model: string;
  cache_ttl_secs: number;
  auto_index: boolean;
}>;

/** POST JSON; throws the server's message when it reports failure. */
/** A failed request, with the server's reply for errors that carry more than a message. */
export class ApiError extends Error {
  constructor(
    message: string,
    readonly body: Record<string, unknown>,
  ) {
    super(message);
  }
}

async function post<T = { ok: boolean; message: string }>(path: string, body: unknown, signal?: AbortSignal): Promise<T> {
  const r = await fetch(path, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body), signal });
  const out = await r.json();
  if (out && out.ok === false) throw new ApiError(out.message ?? `${path}: ${r.status}`, out);
  return out as T;
}

/** Sent back when an HTTPS clone needs an access token. */
export interface NeedsToken {
  host: string;
  username: string;
}

export interface RepoRow {
  path: string;
  name: string;
  exists: boolean;
  branch: string | null;
  your_branch: string | null;
  remote: string | null;
  /** A repository comindly clones and keeps itself. */
  managed: boolean;
  cloned: boolean;
  symbols: number | null;
  indexed_commit: string | null;
}

/** A repository listed by GitHub or GitLab. */
export interface RemoteRepo {
  name: string;
  full_path: string;
  clone_url: string;
  description: string | null;
  default_branch: string | null;
  archived: boolean;
  fork: boolean;
  last_activity: string | null;
  size_kib: number | null;
  listed: boolean;
}

/** A repository in the index with no folder in your list. */
export interface UnlistedRepo {
  name: string;
  symbols: number | null;
  commit: string | null;
  remote: string | null;
}

export interface ScanHit {
  path: string;
  name: string;
  listed: boolean;
  remote: string | null;
}

export interface IndexJob {
  running: boolean;
  cancelled: boolean;
  ok: boolean | null;
  lines: string[];
  started: string | null;
  finished: string | null;
}

declare global {
  interface Window {
    ipc?: { postMessage: (m: string) => void };
    __comindlyPicked?: (path: string | null) => void;
  }
}

/** A native folder dialog when running in the desktop app; `undefined` in a browser. */
export const pickFolder = window.ipc
  ? () =>
      new Promise<string | null>((resolve) => {
        window.__comindlyPicked = resolve;
        window.ipc!.postMessage("pick-folder");
      })
  : undefined;

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

export type Role = "admin" | "member";

/** Who is signed in; `local` when comindly runs on this machine without members. */
export interface Me {
  mode: "local" | "server";
  signed_in?: boolean;
  email?: string | null;
  role?: Role | null;
  /** Everything by email, or everything by codes admins hand out. */
  sign_in?: SignIn;
  public_url?: string | null;
}

export type SignIn = "email" | "code";

export interface Member {
  email: string;
  role: Role;
  /** An admin by the deployment's settings: cannot be changed or removed here. */
  fixed: boolean;
  created: string;
  last_seen: string | null;
}

export interface Invite {
  email: string;
  role: Role;
  created_by: string;
  expires: string;
}

export interface AccessToken {
  id: string;
  name: string;
  created: string;
  last_used: string | null;
}

/** A token or key the server uses; its value never reaches the browser. */
export interface SecretState {
  name: string;
  label: string;
  source: "deployment" | "stored" | "none";
  hint?: string | null;
  updated?: string | null;
  updated_by?: string | null;
}

type Done = { ok: boolean; message: string };

export const api = {
  me: () => get<Me>("/api/auth/me"),
  requestLink: (email: string) => post("/api/auth/link", { email }),
  redeem: (token: string) => post("/api/auth/redeem", { token }),
  signInCode: (email: string, code: string) => post<Done & { code: string | null }>("/api/auth/code", { email, code }),
  resetOwnCode: () => post<Done & { code: string }>("/api/auth/reset-code", {}),
  signOut: (everywhere = false) => post(`/api/auth/signout${everywhere ? "?everywhere=true" : ""}`, {}),
  members: () => get<{ members: Member[]; invites: Invite[]; sign_in: SignIn }>("/api/members"),
  invite: (email: string, role: Role) => post<Done & { code?: string }>("/api/members/invite", { email, role }),
  resetCode: (email: string) => post<Done & { code: string }>("/api/members/reset", { email }),
  setRole: (email: string, role: Role) => post("/api/members/role", { email, role }),
  removeMember: (email: string) => post("/api/members/remove", { email }),
  revokeInvite: (email: string) => post("/api/members/revoke", { email }),
  tokens: () => get<{ tokens: AccessToken[] }>("/api/tokens"),
  createToken: (name: string) => post<Done & { token: string }>("/api/tokens", { name }),
  revokeToken: (id: string) => post("/api/tokens/revoke", { id }),
  secrets: () => get<{ secrets: SecretState[]; can_store: boolean }>("/api/secrets"),
  setSecret: (name: string, value: string) => post("/api/secrets", { name, value }),
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
  settings: () => get<Settings>("/api/settings"),
  repos: () => get<{ repos: RepoRow[]; unlisted: UnlistedRepo[]; team_missing: string[]; root: string | null }>("/api/repos"),
  remoteRepos: (url: string, auth?: { username: string; token: string }, signal?: AbortSignal) =>
    post<{ ok: boolean; provider: string; host: string; owner: string; repos: RemoteRepo[] }>("/api/repos/remote/list", { url, ...auth }, signal),
  addRemoteRepos: (urls: string[], share: boolean) => post("/api/repos/remote/add", { urls, share }),
  changeRepos: (c: { add?: string[]; remove?: string[] }) => post("/api/repos", c),
  scanRepos: (dir: string, signal?: AbortSignal) => post<{ ok: boolean; found: ScanHit[] }>("/api/repos/scan", { dir }, signal),
  cloneRepo: (url: string, dir: string, auth?: { username: string; token: string }) => post("/api/repos/clone", { url, dir, ...auth }),
  flows: () => get<FlowRow[]>("/api/flows"),
  flow: (id: string) => get<FlowDetail>(`/api/flow?id=${encodeURIComponent(id)}`),
  source: (repo: string, at: string) => get<SourceLines>(`/api/source?repo=${encodeURIComponent(repo)}&at=${encodeURIComponent(at)}`),
  indexJob: () => get<IndexJob>("/api/index"),
  startIndex: (llm: boolean, drop_unlisted = false) => post("/api/index", { llm, drop_unlisted }),
  cancelIndex: () => post("/api/index/cancel", {}),
  indexWatch: () => get<{ enabled: boolean; last: { checked: string | null; result: string } }>("/api/index/watch"),
  team: () => get<Team>("/api/team"),
  saveTeam: (settings: TeamSettings, version: number | null) => post("/api/team", { settings, version }),
  saveKey: (key: string) => post<{ ok: boolean; message: string }>("/api/team/key", { key }),
  join: (index: string) => post("/api/join", { index }),
  createTeam: (name: string, index: string) => post("/api/team/create", { name, index }),
  saveSettings: (c: SettingsChange) => post("/api/settings", c),
  testIndex: (index_dir: string) => post("/api/settings/test", { index_dir }),
  context: async (focus: string) => {
    const r = await fetch(`/api/context?focus=${encodeURIComponent(focus)}`);
    return r.text();
  },
  node: (focus: string) => get<NodeDetail>(`/api/node?focus=${encodeURIComponent(focus)}`),
  remember: async (r: {
    kind: string;
    on: string[];
    title: string;
    body: string;
    trigger: string;
    evidence: string[];
    fields: Record<string, string>;
  }) => {
    const res = await fetch("/api/remember", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(r),
    });
    const out = (await res.json()) as { ok: boolean; id?: string; message?: string };
    if (!out.ok) throw new Error(out.message ?? "not recorded");
    return out;
  },
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
