import { AlertTriangle, CheckCircle2, ChevronRight, XCircle } from "lucide-react";
import { useState } from "react";
import type { IndexEvent } from "@/lib/api";
import { cn } from "@/lib/utils";

const time = (e: IndexEvent) => (e.timestamp ? new Date(e.timestamp).toLocaleTimeString() : "");
const fmt = (n: number) => n.toLocaleString();

/** The step in progress, if any: the latest progress event after the latest step. */
export function currentProgress(events: IndexEvent[]): IndexEvent | null {
  for (let i = events.length - 1; i >= 0; i--) {
    const e = events[i];
    if (e.kind === "progress") return e;
    if (e.kind === "step" || e.kind === "done") return null;
  }
  return null;
}

/** A short status for the header: the current step, with its count when there is one. */
export function shortStatus(events: IndexEvent[]): string {
  const p = currentProgress(events);
  if (p && p.total) return `${p.step ?? p.message} ${fmt(p.done ?? 0)}/${fmt(p.total)}`;
  return [...events].reverse().find((e) => e.kind === "step")?.message ?? "";
}

function Progress({ e }: { e: IndexEvent }) {
  const pct = e.total ? Math.min(100, ((e.done ?? 0) / e.total) * 100) : 0;
  return (
    <div className="rounded-lg border bg-card p-3">
      <div className="flex flex-wrap items-baseline justify-between gap-2 text-sm">
        <span className="font-medium">{e.step ?? e.message}</span>
        <span className="nums font-mono text-xs text-muted-foreground">
          {fmt(e.done ?? 0)} / {fmt(e.total ?? 0)}
        </span>
      </div>
      <div className="mt-2 h-1.5 overflow-hidden rounded-full bg-muted">
        <div className="h-full rounded-full bg-brand transition-all" style={{ width: `${pct}%` }} />
      </div>
      {e.tokens_in != null && (
        <div className="nums mt-2 font-mono text-[11px] text-muted-foreground">
          {fmt(e.tokens_in)} tokens in · {fmt(e.tokens_out ?? 0)} out
          {e.usd != null && ` · ≈ ${e.usd < 0.01 ? "< $0.01" : `$${e.usd.toFixed(2)}`}`}
        </div>
      )}
    </div>
  );
}

function Line({ e }: { e: IndexEvent }) {
  if (e.kind === "step")
    return (
      <div className="mt-2 flex items-center gap-1.5 font-medium first:mt-0">
        <ChevronRight className="size-3.5 text-muted-foreground" />
        {e.message}
        <span className="ml-auto font-mono text-[10.5px] font-normal text-muted-foreground">{time(e)}</span>
      </div>
    );
  const icon =
    e.level === "error" ? (
      <XCircle className="mt-0.5 size-3.5 shrink-0 text-destructive" />
    ) : e.level === "warn" ? (
      <AlertTriangle className="mt-0.5 size-3.5 shrink-0 text-warning" />
    ) : e.kind === "done" ? (
      <CheckCircle2 className="mt-0.5 size-3.5 shrink-0 text-brand" />
    ) : (
      <span className="w-3.5 shrink-0" />
    );
  return (
    <div
      className={cn(
        "flex items-start gap-1.5 pl-5",
        e.kind === "note" && "text-muted-foreground",
        e.level === "warn" && "text-warning",
        e.level === "error" && "text-destructive",
      )}
    >
      {icon}
      <span className="min-w-0 [overflow-wrap:anywhere]">{e.message}</span>
    </div>
  );
}

/** The indexing log: steps, results and warnings, with a live bar for the step in progress. */
export function IndexLog({ events, running }: { events: IndexEvent[]; running: boolean }) {
  const [full, setFull] = useState(false);
  const lines = events.filter((e) => e.kind !== "progress");
  const shown = full ? lines : lines.slice(-14);
  const progress = running ? currentProgress(events) : null;
  if (events.length === 0) return null;
  return (
    <div className="flex flex-col gap-2">
      {progress && <Progress e={progress} />}
      <div className="max-h-96 overflow-auto rounded-lg border bg-muted/30 p-3 text-[12.5px] leading-relaxed">
        {shown.map((e, i) => (
          <Line key={i} e={e} />
        ))}
      </div>
      {lines.length > 14 && (
        <button className="self-start text-xs text-muted-foreground hover:text-foreground" onClick={() => setFull((f) => !f)}>
          {full ? "Show less" : `Show all ${lines.length} entries`}
        </button>
      )}
    </div>
  );
}
