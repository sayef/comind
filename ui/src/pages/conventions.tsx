import { useQuery } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { RepoDot } from "@/components/symbol";
import { Badge } from "@/components/ui/badge";
import { Card } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { api } from "@/lib/api";
import { cn } from "@/lib/utils";

export function ConventionsPage() {
  const all = useQuery({ queryKey: ["conventions"], queryFn: api.conventions });
  const repos = useMemo(() => [...new Set((all.data ?? []).map((c) => c.repo))].sort((a, b) => (a === "*" ? -1 : b === "*" ? 1 : a.localeCompare(b))), [all.data]);
  const [repo, setRepo] = useState<string | null>(null);
  const current = repo ?? repos[0];
  const rows = (all.data ?? []).filter((c) => c.repo === current);
  const byDim = useMemo(() => {
    const m = new Map<string, typeof rows>();
    for (const c of rows) m.set(c.dimension, [...(m.get(c.dimension) ?? []), c]);
    return [...m];
  }, [rows]);

  return (
    <div className="flex flex-col gap-5">
      <div>
        <h1 className="text-2xl font-semibold tracking-tight">Conventions</h1>
        <p className="mt-1 text-sm text-muted-foreground">Measured from the code: how each repository names, structures, logs, configures and tests things.</p>
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
            {r !== "*" && <RepoDot repo={r} />}
            {r === "*" ? "Org baseline" : r}
          </button>
        ))}
      </div>
      {all.isLoading && <Skeleton className="h-64 rounded-xl" />}
      <div className="grid gap-4 lg:grid-cols-2">
        {byDim.map(([dim, items]) => (
          <Card key={dim} className="p-4">
            <h2 className="text-xs font-semibold uppercase tracking-wider text-muted-foreground">{dim}</h2>
            <div className="mt-3 flex flex-col gap-4">
              {items.map((c) => {
                const pct = c.total ? Math.round((c.support / c.total) * 100) : 0;
                return (
                  <div key={c.key}>
                    <p className="text-sm leading-snug">{c.rule}</p>
                    <div className="mt-2 flex items-center gap-2">
                      <div className="h-1.5 flex-1 overflow-hidden rounded-full bg-muted">
                        <div className={cn("h-full rounded-full", pct >= 80 ? "bg-brand" : "bg-warning")} style={{ width: `${pct}%` }} />
                      </div>
                      <span className="font-mono text-[11px] tabular-nums text-muted-foreground">
                        {c.support}/{c.total}
                      </span>
                    </div>
                    {c.exemplars.length > 0 && (
                      <div className="mt-2 flex flex-wrap gap-1">
                        {c.exemplars.slice(0, 3).map((x) => (
                          <Badge key={x} variant="outline" className="font-mono">
                            {x}
                          </Badge>
                        ))}
                      </div>
                    )}
                  </div>
                );
              })}
            </div>
          </Card>
        ))}
      </div>
    </div>
  );
}
