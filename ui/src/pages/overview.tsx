import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowUpRight, Boxes, GitBranch, History, Network, Workflow } from "lucide-react";
import { useMemo, useState } from "react";
import { Link, useNavigate } from "react-router";
import { GraphView } from "@/components/graph-view";
import { RepoDot } from "@/components/symbol";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { Slider } from "@/components/ui/slider";
import { Table, Td, Th, Tr } from "@/components/ui/table";
import { Outcome } from "@/components/notice";
import { api, attention, type OrgMap } from "@/lib/api";

function Stat({ icon: Icon, label, value, hint }: { icon: typeof Boxes; label: string; value: string; hint?: string }) {
  return (
    <Card className="p-4">
      <div className="flex items-center gap-2 text-xs text-muted-foreground">
        <Icon className="size-3.5" />
        {label}
      </div>
      <div className="mt-2 text-2xl font-semibold tabular-nums tracking-tight">{value}</div>
      {hint && <div className="mt-0.5 text-xs text-muted-foreground">{hint}</div>}
    </Card>
  );
}

/** Repos that depend on `repo`, directly or transitively, with their hop count. */
function dependents(map: OrgMap, repo: string) {
  const seen = new Map<string, number>();
  const queue: [string, number][] = [[repo, 0]];
  while (queue.length) {
    const [r, d] = queue.shift()!;
    for (const e of map.edges) {
      if (e.dst === r && e.src !== repo && !seen.has(e.src)) {
        seen.set(e.src, d + 1);
        queue.push([e.src, d + 1]);
      }
    }
  }
  return seen;
}

function linkDiff(then: OrgMap, now: OrgMap) {
  const key = (e: OrgMap["edges"][number]) => `${e.src} → ${e.dst} (${e.kind})`;
  const a = new Set(then.edges.map(key));
  const b = new Set(now.edges.map(key));
  return { added: [...b].filter((k) => !a.has(k)), gone: [...a].filter((k) => !b.has(k)) };
}

/** Where a team's index lives: a folder or a bucket location. */
function Location({ index, setIndex }: { index: string; setIndex: (v: string) => void }) {
  return <Input value={index} onChange={(e) => setIndex(e.target.value)} placeholder="s3://, gs://, az:// or a folder" className="font-mono text-xs" />;
}

/** First run: use the team's index. */
function JoinTeam() {
  const qc = useQueryClient();
  const [index, setIndex] = useState("");
  const join = useMutation({ mutationFn: () => api.join(index), meta: { inline: true }, onSuccess: () => qc.invalidateQueries() });
  return (
    <Card className="flex flex-col p-4 text-left">
      <div className="text-sm font-medium">Join a team</div>
      <p className="mt-1 text-xs text-muted-foreground">
        Where your team's index lives. Bucket access uses the credentials this machine already has.
      </p>
      <div className="mt-3 flex flex-1 flex-col gap-2">
        <Location index={index} setIndex={setIndex} />
        <div className="mt-auto">
          <Button variant="brand" disabled={!index.trim()} loading={join.isPending} onClick={() => join.mutate()}>
            Join
          </Button>
        </div>
        <Outcome error={join.error} />
      </div>
    </Card>
  );
}

/** First run: start a team at an empty folder or bucket location; colleagues then join it. */
function CreateTeam() {
  const qc = useQueryClient();
  const navigate = useNavigate();
  const [name, setName] = useState("");
  const [index, setIndex] = useState("");
  const create = useMutation({
    mutationFn: () => api.createTeam(name, index),
    meta: { inline: true },
    onSuccess: () => (qc.invalidateQueries(), navigate("/settings/repositories")),
  });
  return (
    <Card className="flex flex-col p-4 text-left">
      <div className="text-sm font-medium">Create a team</div>
      <p className="mt-1 text-xs text-muted-foreground">
        Pick an empty folder or bucket location everyone can reach. You add repositories next; colleagues join with the same location.
      </p>
      <div className="mt-3 flex flex-1 flex-col gap-2">
        <Input value={name} onChange={(e) => setName(e.target.value)} placeholder="Team name, e.g. Platform" />
        <Location index={index} setIndex={setIndex} />
        <div className="mt-auto">
          <Button variant="brand" disabled={!name.trim() || !index.trim()} loading={create.isPending} onClick={() => create.mutate()}>
            Create team
          </Button>
        </div>
        <Outcome error={create.error} />
      </div>
    </Card>
  );
}

export function OverviewPage() {
  const overview = useQuery({ queryKey: ["overview"], queryFn: api.overview });
  const ledger = useQuery({ queryKey: ["ledger"], queryFn: api.ledger });
  const latest = useQuery({ queryKey: ["orgmap"], queryFn: () => api.orgmap() });
  const versions = useQuery({ queryKey: ["versions"], queryFn: api.versions });
  const last = (versions.data?.length ?? 1) - 1;
  const [at, setAt] = useState<number | null>(null);
  const past = useQuery({
    queryKey: ["orgmap", at],
    queryFn: () => api.orgmap(at!),
    enabled: at !== null && at !== last,
  });
  const map = at !== null && at !== last ? past.data : latest.data;
  const [selected, setSelected] = useState<string | null>(null);
  const reach = useMemo(() => (map && selected ? dependents(map, selected) : null), [map, selected]);
  const highlight = useMemo(() => (reach ? new Set(reach.keys()) : null), [reach]);
  const graph = useMemo(
    () =>
      map
        ? {
            nodes: map.nodes.map((n) => ({ id: n.id, label: n.name, repo: n.repo })),
            edges: map.edges.map((e) => ({ src: e.src, dst: e.dst, kind: e.kind, weight: e.weight })),
          }
        : null,
    [map],
  );
  const diff = at !== null && at !== last && past.data && latest.data ? linkDiff(past.data, latest.data) : null;
  const o = overview.data;
  const team = useQuery({ queryKey: ["team"], queryFn: api.team, retry: false });
  const teamName = team.data?.version != null ? (team.data.settings.name ?? "your team") : null;
  const [switching, setSwitching] = useState(false);

  if (o?.problem && teamName && !switching) {
    return (
      <div className="mx-auto flex max-w-xl flex-col items-center gap-4 py-16 text-center">
        <img src="/logo.svg" alt="" className="size-12 dark:hidden" />
        <img src="/logo-dark.svg" alt="" className="hidden size-12 dark:block" />
        <h1 className="text-xl font-semibold">You're in {teamName}</h1>
        <p className="text-sm text-muted-foreground">
          Nothing is indexed yet. Add the team's repositories and index them; everyone in {teamName} then sees the same graph.
        </p>
        <div className="flex flex-wrap justify-center gap-2">
          <Button variant="brand" asChild>
            <Link to="/settings/repositories">Add repositories</Link>
          </Button>
          <Button variant="outline" asChild>
            <Link to="/settings">Team settings</Link>
          </Button>
        </div>
        <button className="text-xs text-muted-foreground underline underline-offset-2" onClick={() => setSwitching(true)}>
          Join or create a different team
        </button>
      </div>
    );
  }

  if (o?.problem) {
    return (
      <div className="mx-auto flex max-w-3xl flex-col items-center gap-4 py-16 text-center">
        <img src="/logo.svg" alt="" className="size-12 dark:hidden" />
        <img src="/logo-dark.svg" alt="" className="hidden size-12 dark:block" />
        <h1 className="text-xl font-semibold">Welcome to comindly</h1>
        <p className="text-sm text-muted-foreground">Join your team, start one, or index just for yourself.</p>
        <div className="grid w-full gap-3 sm:grid-cols-2">
          <JoinTeam />
          <CreateTeam />
        </div>
        <p className="text-xs text-muted-foreground">
          Just for yourself? <Link to="/settings/repositories" className="underline underline-offset-2">Add repositories</Link> and index them
          here, or change the location in <Link to="/settings" className="underline underline-offset-2">Settings</Link>.
        </p>
        <details className="text-left text-xs text-muted-foreground">
          <summary className="cursor-pointer">Details</summary>
          <p className="mt-2">
            Looked in <span className="font-mono [overflow-wrap:anywhere]">{o.root}</span>
          </p>
          <pre className="mt-1 whitespace-pre-wrap [overflow-wrap:anywhere]">{o.problem}</pre>
        </details>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-6">
      <div>
        <h1 className="text-2xl font-semibold tracking-tight">Overview</h1>
        <p className="mt-1 text-sm text-muted-foreground">How the indexed repositories depend on each other, in code and through service contracts.</p>
      </div>

      <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
        {o ? (
          <>
            <Stat icon={Boxes} label="Repositories" value={String(o.repos.length)} hint={`${o.symbols.toLocaleString()} symbols`} />
            <Stat icon={GitBranch} label="Dependency edges" value={o.edges.toLocaleString()} hint={`${o.cross_repo_edges.toLocaleString()} cross-repo`} />
            <Stat icon={Network} label="Service contracts" value={String(o.contracts)} hint={`${o.linked_contracts} with both sides indexed`} />
            <Stat
              icon={History}
              label="Ledger records"
              value={String(Object.values(o.ledger).reduce((a, b) => a + b, 0))}
              hint={`${(ledger.data ?? []).filter((e) => attention(e) < 3).length} need review`}
            />
          </>
        ) : (
          [0, 1, 2, 3].map((i) => <Skeleton key={i} className="h-[98px] rounded-xl" />)
        )}
      </div>

      <Card>
        <CardHeader className="flex-row items-start justify-between gap-4">
          <div>
            <CardTitle className="flex items-center gap-2">
              <Workflow className="size-4" /> Org map
            </CardTitle>
            <CardDescription>
              Arrows point from dependent to dependency; amber edges are service contracts. Click a repository to see what a change there can reach.
            </CardDescription>
          </div>
          {selected && (
            <button className="text-xs text-muted-foreground hover:text-foreground" onClick={() => setSelected(null)}>
              Clear
            </button>
          )}
        </CardHeader>
        <CardContent>
          <div className="overflow-hidden rounded-lg border bg-muted/30">
            {graph ? (
              <GraphView
                nodes={graph.nodes}
                edges={graph.edges}
                highlight={highlight}
                onPick={(id) => setSelected((s) => (s === id ? null : id))}
                className="h-[520px] w-full"
              />
            ) : (
              <Skeleton className="h-[520px] rounded-none" />
            )}
          </div>
          {selected && reach && (
            <div className="mt-3 flex flex-wrap items-center gap-1.5 text-sm">
              <span className="font-medium">{selected}</span>
              <span className="text-muted-foreground">{reach.size ? "— a change can reach" : "— no other indexed repository depends on it"}</span>
              {[...reach].map(([r, d]) => (
                <Badge key={r} variant="warning">
                  {r} · {d} hop{d > 1 ? "s" : ""}
                </Badge>
              ))}
            </div>
          )}
          {versions.data && versions.data.length > 1 && (
            <div className="mt-4 rounded-lg border p-3">
              <div className="flex items-center justify-between text-xs">
                <span className="font-medium">Index version</span>
                <span className="font-mono text-muted-foreground">
                  {(versions.data[at ?? last] ?? "").slice(0, 16).replace("T", " ")}
                  {(at ?? last) === last ? " · latest" : ` · ${(at ?? last) + 1} of ${last + 1}`}
                </span>
              </div>
              <Slider className="mt-3" min={0} max={last} step={1} value={[at ?? last]} onValueChange={([v]) => setAt(v)} />
              {diff && (
                <div className="mt-3 flex flex-wrap gap-1.5 text-xs">
                  {diff.added.length + diff.gone.length === 0 && <span className="text-muted-foreground">No repository links changed since then.</span>}
                  {diff.added.map((k) => (
                    <Badge key={k} variant="brand">
                      + {k}
                    </Badge>
                  ))}
                  {diff.gone.map((k) => (
                    <Badge key={k} variant="danger">
                      − {k}
                    </Badge>
                  ))}
                </div>
              )}
            </div>
          )}
        </CardContent>
      </Card>

      <div className="grid gap-6 lg:grid-cols-2">
        <Card>
          <CardHeader>
            <CardTitle>Repositories</CardTitle>
            <CardDescription>Indexed symbols per repository.</CardDescription>
          </CardHeader>
          <CardContent className="flex flex-col gap-2.5">
            {o?.repos
              .slice()
              .sort((a, b) => b.symbols - a.symbols)
              .map((r) => {
                const max = Math.max(...o.repos.map((x) => x.symbols));
                return (
                  <div key={r.repo} className="flex items-center gap-3 text-sm">
                    <RepoDot repo={r.repo} />
                    <span className="w-40 truncate">{r.repo}</span>
                    <div className="h-1.5 flex-1 overflow-hidden rounded-full bg-muted">
                      <div className="h-full rounded-full bg-brand/70" style={{ width: `${(r.symbols / max) * 100}%` }} />
                    </div>
                    <span className="w-14 text-right font-mono text-xs tabular-nums text-muted-foreground">{r.symbols.toLocaleString()}</span>
                  </div>
                );
              })}
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="flex-row items-start justify-between">
            <div>
              <CardTitle>Version skew</CardTitle>
              <CardDescription>Internal packages pinned to a different version than their publisher.</CardDescription>
            </div>
            <Link to="/packages" className="flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground">
              Packages <ArrowUpRight className="size-3" />
            </Link>
          </CardHeader>
          <CardContent>
            {o && o.skew.length === 0 && <p className="text-sm text-muted-foreground">Every consumer pins its publisher's version.</p>}
            {o && o.skew.length > 0 && (
              <Table>
                <thead>
                  <tr className="border-b">
                    <Th>Package</Th>
                    <Th>Consumer</Th>
                    <Th>Pinned</Th>
                    <Th>Published</Th>
                  </tr>
                </thead>
                <tbody>
                  {o.skew.map((s) => (
                    <Tr key={`${s.package}-${s.consumer}`}>
                      <Td className="font-medium">{s.package}</Td>
                      <Td>{s.consumer}</Td>
                      <Td className="font-mono text-xs">{s.locked}</Td>
                      <Td className="font-mono text-xs text-muted-foreground">
                        {s.published} <span className="text-[10px]">({s.publisher})</span>
                      </Td>
                    </Tr>
                  ))}
                </tbody>
              </Table>
            )}
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
