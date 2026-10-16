import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Check, ShieldAlert, ShieldCheck, X } from "lucide-react";
import { useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { api, type Entry, isChallenged, when } from "@/lib/api";
import { cn } from "@/lib/utils";

const STATUS: Record<string, "brand" | "warning" | "danger" | "outline"> = {
  active: "brand",
  moved: "brand",
  stale: "warning",
  orphaned: "danger",
  superseded: "outline",
  retracted: "outline",
};

export function TrustBadge({ trust }: { trust: string }) {
  const high = trust === "T3" || trust === "T4";
  return (
    <Badge variant={high ? "brand" : trust === "T0" ? "danger" : "default"} title="Trust level">
      {trust}
    </Badge>
  );
}

export function EntryCard({ entry, writable, compact = false }: { entry: Entry; writable?: boolean; compact?: boolean }) {
  const r = entry.record;
  const qc = useQueryClient();
  const [retracting, setRetracting] = useState(false);
  const [reason, setReason] = useState("");
  const review = useMutation({
    mutationFn: ({ action, why }: { action: "confirm" | "retract"; why?: string }) => api.review(r.id, action, why),
    onSuccess: () => {
      setRetracting(false);
      qc.invalidateQueries({ queryKey: ["ledger"] });
      qc.invalidateQueries({ queryKey: ["overview"] });
      qc.invalidateQueries({ queryKey: ["node"] });
    },
  });
  const retired = entry.status === "retracted" || entry.status === "superseded";
  const challenged = isChallenged(entry);
  const fields = Object.entries(r.fields).filter(([k]) => k !== "at");

  return (
    <Card className={cn("p-4", retired && "opacity-60")}>
      <div className="flex flex-wrap items-center gap-1.5 text-xs text-muted-foreground">
        <Badge variant="outline" className="capitalize">
          {r.kind}
        </Badge>
        <TrustBadge trust={entry.trust} />
        <Badge variant={STATUS[entry.status] ?? "outline"}>{entry.status}</Badge>
        {challenged && <Badge variant="warning">challenged</Badge>}
        {entry.verified === true && <ShieldCheck className="size-3.5 text-brand" aria-label="signature verified" />}
        {entry.verified === false && <ShieldAlert className="size-3.5 text-danger" aria-label="signature invalid" />}
        <span className="ml-auto">
          {when(entry).slice(0, 10)} · {r.provenance.author}
        </span>
      </div>
      <h4 className="mt-2.5 font-medium leading-snug">{r.title}</h4>
      {!compact && r.body && <p className="mt-1.5 text-sm text-muted-foreground">{r.body}</p>}
      {!compact && (r.trigger || fields.length > 0 || r.evidence.length > 0) && (
        <dl className="mt-3 grid gap-1 text-xs">
          {r.trigger && (
            <div className="flex gap-2">
              <dt className="w-24 shrink-0 text-muted-foreground">Relevant when</dt>
              <dd>{r.trigger}</dd>
            </div>
          )}
          {fields.map(([k, v]) => (
            <div key={k} className="flex gap-2">
              <dt className="w-24 shrink-0 capitalize text-muted-foreground">{k}</dt>
              <dd className="font-mono">{typeof v === "string" ? v : JSON.stringify(v)}</dd>
            </div>
          ))}
          {r.evidence.length > 0 && (
            <div className="flex gap-2">
              <dt className="w-24 shrink-0 text-muted-foreground">Evidence</dt>
              <dd className="font-mono break-all">{r.evidence.join(", ")}</dd>
            </div>
          )}
        </dl>
      )}
      <div className="mt-3 flex flex-wrap gap-1.5">
        {r.anchors.map((a) => (
          <span key={a.symbol} className="rounded bg-muted px-1.5 py-0.5 font-mono text-[11px]" title={`${a.path}:${a.line}`}>
            {a.repo}:{a.name}
          </span>
        ))}
      </div>
      {writable && !retired && !compact && (
        <div className="mt-4 flex flex-wrap items-center gap-2">
          {retracting ? (
            <>
              <Input
                autoFocus
                value={reason}
                onChange={(e) => setReason(e.target.value)}
                placeholder="Why is this no longer true?"
                className="h-8 max-w-sm text-xs"
              />
              <Button size="sm" variant="outline" onClick={() => review.mutate({ action: "retract", why: reason })}>
                Retract
              </Button>
              <Button size="sm" variant="ghost" onClick={() => setRetracting(false)}>
                Cancel
              </Button>
            </>
          ) : (
            <>
              <Button size="sm" variant="brand" onClick={() => review.mutate({ action: "confirm" })} disabled={review.isPending}>
                <Check /> Confirm
              </Button>
              <Button size="sm" variant="outline" onClick={() => setRetracting(true)}>
                <X /> Retract
              </Button>
            </>
          )}
          {review.data && <span className="text-xs text-muted-foreground">{review.data.message}</span>}
        </div>
      )}
    </Card>
  );
}
