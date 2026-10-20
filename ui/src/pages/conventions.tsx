import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Pencil, Plus, RotateCcw, Trash2 } from "lucide-react";
import { useMemo, useState } from "react";
import { useSearchParams } from "react-router";
import { RepoDot } from "@/components/symbol";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Dialog, DialogContent, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { Tooltip } from "@/components/ui/tooltip";
import { api, type Convention, type ConventionChange } from "@/lib/api";
import { cn, sentence } from "@/lib/utils";

const ORG = "*";
const scopeName = (r: string) => (r === ORG ? "Org baseline" : r);
const textarea =
  "min-h-20 w-full rounded-md border bg-card px-3 py-2 text-sm shadow-xs outline-none focus-visible:ring-2 focus-visible:ring-ring";

function useChange() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (c: ConventionChange) => api.changeConvention(c),
    meta: { inline: true },
    onSuccess: () => qc.invalidateQueries({ queryKey: ["conventions"] }),
  });
}

function Rule({ c, writable }: { c: Convention; writable: boolean }) {
  const change = useChange();
  const [editing, setEditing] = useState(false);
  const [text, setText] = useState(c.rule);
  const pct = c.total ? Math.round((c.support / c.total) * 100) : 0;

  if (editing) {
    return (
      <div className="flex flex-col gap-2">
        <textarea className={textarea} value={text} onChange={(e) => setText(e.target.value)} autoFocus />
        {change.error && <p className="text-xs text-danger [overflow-wrap:anywhere]">{sentence(change.error.message)}</p>}
        <div className="flex gap-2">
          <Button
            size="sm"
            variant="brand"
            disabled={!text.trim()} loading={change.isPending}
            onClick={() => change.mutate({ action: "edit", id: c.id, rule: text }, { onSuccess: () => setEditing(false) })}
          >
            Save
          </Button>
          <Button size="sm" variant="ghost" onClick={() => setEditing(false)}>
            Cancel
          </Button>
        </div>
      </div>
    );
  }

  return (
    <div className="group">
      <div className="flex items-start gap-2">
        <p className="flex-1 text-sm leading-snug">
          {c.rule}
          {c.origin === "manual" && (
            <Badge variant="brand" className="ml-2 align-middle">
              Team
            </Badge>
          )}
          {c.origin === "edited" && (
            <Badge variant="outline" className="ml-2 align-middle">
              Edited
            </Badge>
          )}
        </p>
        {writable && (
          <div className="flex shrink-0 gap-0.5 opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100">
            <Tooltip content="Edit">
              <Button size="icon" variant="ghost" className="size-7" onClick={() => (setText(c.rule), setEditing(true))} aria-label="Edit">
                <Pencil />
              </Button>
            </Tooltip>
            {c.origin === "edited" && (
              <Tooltip content="Back to the measured rule">
                <Button size="icon" variant="ghost" className="size-7" disabled={change.isPending} onClick={() => change.mutate({ action: "restore", id: c.id })} aria-label="Restore">
                  <RotateCcw />
                </Button>
              </Tooltip>
            )}
            <Tooltip content={c.origin === "manual" ? "Delete" : "Hide (can be restored)"}>
              <Button size="icon" variant="ghost" className="size-7" disabled={change.isPending} onClick={() => change.mutate({ action: "hide", id: c.id })} aria-label="Remove">
                <Trash2 />
              </Button>
            </Tooltip>
          </div>
        )}
      </div>
      {c.origin !== "manual" && (
        <div className="mt-2 flex items-center gap-2">
          <div className="h-1.5 flex-1 overflow-hidden rounded-full bg-muted">
            <div className={cn("h-full rounded-full", pct >= 80 ? "bg-brand" : "bg-warning")} style={{ width: `${pct}%` }} />
          </div>
          <span className="font-mono text-[11px] tabular-nums text-muted-foreground">
            {c.support}/{c.total}
          </span>
        </div>
      )}
      {c.exemplars.length > 0 && (
        <div className="mt-2 flex flex-wrap gap-1">
          {c.exemplars.slice(0, 3).map((x) => (
            <Badge key={x} variant="outline" className="max-w-full font-mono" title={x}>
              <span className="min-w-0 truncate">{x}</span>
            </Badge>
          ))}
        </div>
      )}
    </div>
  );
}

function AddDialog({ open, onOpenChange, scopes, scope, dimensions }: { open: boolean; onOpenChange: (o: boolean) => void; scopes: string[]; scope: string; dimensions: string[] }) {
  const change = useChange();
  const [repo, setRepo] = useState(scope);
  const [dimension, setDimension] = useState("");
  const [rule, setRule] = useState("");
  const submit = () =>
    change.mutate(
      { action: "add", repo, dimension: dimension.trim() || "team", rule },
      { onSuccess: () => (setRule(""), setDimension(""), onOpenChange(false)) },
    );
  return (
    <Dialog open={open} onOpenChange={(o) => (o && setRepo(scope), onOpenChange(o))}>
      <DialogContent className="p-5">
        <DialogTitle className="text-base font-semibold">Add a convention</DialogTitle>
        <p className="mt-1 text-sm text-muted-foreground">Agents see it through the MCP guide, marked as set by the team.</p>
        <div className="mt-4 flex flex-col gap-3">
          <label className="flex flex-col gap-1 text-xs font-medium">
            Applies to
            <select className="h-9 rounded-md border bg-card px-2 text-sm" value={repo} onChange={(e) => setRepo(e.target.value)}>
              {scopes.map((r) => (
                <option key={r} value={r}>
                  {scopeName(r)}
                </option>
              ))}
            </select>
          </label>
          <label className="flex flex-col gap-1 text-xs font-medium">
            Topic
            <Input list="dimensions" value={dimension} onChange={(e) => setDimension(e.target.value)} placeholder="e.g. errors, libraries, testing" />
            <datalist id="dimensions">
              {dimensions.map((d) => (
                <option key={d} value={d} />
              ))}
            </datalist>
          </label>
          <label className="flex flex-col gap-1 text-xs font-medium">
            Rule
            <textarea className={textarea} value={rule} onChange={(e) => setRule(e.target.value)} placeholder="One sentence an agent can follow." />
          </label>
          {change.error && <p className="text-xs text-danger [overflow-wrap:anywhere]">{sentence(change.error.message)}</p>}
          <div className="flex justify-end gap-2">
            <Button variant="ghost" onClick={() => onOpenChange(false)}>
              Cancel
            </Button>
            <Button variant="brand" disabled={!rule.trim()} loading={change.isPending} onClick={submit}>
              Add
            </Button>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}

export function ConventionsPage() {
  const all = useQuery({ queryKey: ["conventions"], queryFn: api.conventions });
  const overview = useQuery({ queryKey: ["overview"], queryFn: api.overview });
  const writable = overview.data?.writable ?? false;
  const change = useChange();
  const items = all.data?.conventions ?? [];
  const repos = useMemo(() => {
    const names = new Set([ORG, ...items.map((c) => c.repo), ...(overview.data?.repos.map((r) => r.repo) ?? [])]);
    return [...names].sort((a, b) => (a === ORG ? -1 : b === ORG ? 1 : a.localeCompare(b)));
  }, [items, overview.data]);
  const [params, setParams] = useSearchParams();
  const repo = params.get("repo");
  const setRepo = (r: string) => setParams(r === ORG ? {} : { repo: r }, { replace: true });
  const [adding, setAdding] = useState(false);
  const current = repo ?? repos[0] ?? ORG;
  const rows = items.filter((c) => c.repo === current);
  const hidden = (all.data?.hidden ?? []).filter((h) => h.repo === current);
  const dimensions = useMemo(() => [...new Set(items.map((c) => c.dimension))].sort(), [items]);
  const byDim = useMemo(() => {
    const m = new Map<string, Convention[]>();
    for (const c of rows) m.set(c.dimension, [...(m.get(c.dimension) ?? []), c]);
    return [...m];
  }, [rows]);

  return (
    <div className="flex flex-col gap-5">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">Conventions</h1>
          <p className="mt-1 text-sm text-muted-foreground">
            Measured from the code, plus the team's own: how each repository names, structures, logs, configures and tests things.
          </p>
        </div>
        {writable && (
          <Button variant="brand" onClick={() => setAdding(true)}>
            <Plus /> Add convention
          </Button>
        )}
      </div>
      <div className="flex flex-wrap gap-1.5">
        {repos.map((r) => (
          <button
            key={r}
            onClick={() => setRepo(r)}
            className={cn(
              "flex items-center gap-1.5 rounded-full border px-3 py-1 text-xs transition-colors",
              r === current ? "border-brand bg-accent text-accent-foreground" : "hover:bg-muted",
            )}
          >
            {r !== ORG && <RepoDot repo={r} />}
            {scopeName(r)}
          </button>
        ))}
      </div>
      {all.isLoading && <Skeleton className="h-64 rounded-xl" />}
      {!all.isLoading && rows.length === 0 && <p className="text-sm text-muted-foreground">No conventions for {scopeName(current)} yet.</p>}
      <div className="grid items-start gap-4 lg:grid-cols-2">
        {byDim.map(([dim, list]) => (
          <Card key={dim} className="min-w-0 p-4">
            <h2 className="text-xs font-semibold uppercase tracking-wider text-muted-foreground">{dim}</h2>
            <div className="mt-3 flex flex-col gap-4">
              {list.map((c) => (
                <Rule key={`${c.id}-${c.rule}`} c={c} writable={writable} />
              ))}
            </div>
          </Card>
        ))}
      </div>
      {hidden.length > 0 && (
        <Card className="p-4">
          <h2 className="text-xs font-semibold uppercase tracking-wider text-muted-foreground">Hidden</h2>
          <div className="mt-3 flex flex-col gap-2">
            {hidden.map((h) => (
              <div key={h.id} className="flex items-center gap-3 text-sm text-muted-foreground">
                <Badge variant="outline">{h.dimension}</Badge>
                <span className="flex-1 line-through decoration-muted-foreground/50">{h.rule}</span>
                {writable && (
                  <Button size="sm" variant="outline" loading={change.isPending && (change.variables as { id?: string } | undefined)?.id === h.id} onClick={() => change.mutate({ action: "restore", id: h.id })}>
                    <RotateCcw /> Restore
                  </Button>
                )}
              </div>
            ))}
          </div>
        </Card>
      )}
      <AddDialog open={adding} onOpenChange={setAdding} scopes={repos} scope={current} dimensions={dimensions} />
    </div>
  );
}
