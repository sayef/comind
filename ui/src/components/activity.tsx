import { useIsMutating, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Loader2, X } from "lucide-react";
import { useEffect, useRef } from "react";
import { Link } from "react-router";
import { toast } from "sonner";
import { api } from "@/lib/api";
import { cn } from "@/lib/utils";

/** What is running right now, in the header's corner, with a way to stop it; and a toast when indexing ends. */
export function Activity() {
  const qc = useQueryClient();
  const job = useQuery({ queryKey: ["index-job"], queryFn: api.indexJob, refetchInterval: (q) => (q.state.data?.running ? 1000 : 10_000) });
  const overview = useQuery({ queryKey: ["overview"], queryFn: api.overview });
  const remote = !!overview.data?.root?.includes("://");
  const aws = useQuery({ queryKey: ["aws"], queryFn: api.aws, enabled: remote, refetchInterval: (q) => (q.state.data?.run?.running ? 2000 : 5 * 60_000) });
  const busy = useIsMutating();
  const stopIndex = useMutation({ mutationFn: api.cancelIndex, meta: { quiet: true }, onSuccess: () => qc.invalidateQueries({ queryKey: ["index-job"] }) });
  const stopSignIn = useMutation({ mutationFn: api.cancelAwsLogin, meta: { quiet: true }, onSuccess: () => qc.invalidateQueries({ queryKey: ["aws"] }) });

  const j = job.data;
  const was = useRef<boolean | undefined>(undefined);
  useEffect(() => {
    if (!j) return;
    if (was.current && !j.running) {
      if (j.cancelled) toast.info("Indexing cancelled");
      else if (j.ok) toast.success("Indexing finished; every page shows the new index");
      else toast.error("Indexing failed", { action: { label: "See the log", onClick: () => window.location.assign("/settings/repositories") } });
      qc.invalidateQueries({ predicate: (q) => q.queryKey[0] !== "index-job" });
    }
    was.current = j.running;
  }, [j, qc]);

  const step = j?.lines.at(-1)?.replace(/^[^A-Za-z0-9]+/, "") ?? "";
  if (j?.running)
    return (
      <Pill>
        <Link to="/settings/repositories" className="flex min-w-0 items-center gap-2" title={step}>
          <Loader2 className="size-3.5 shrink-0 animate-spin text-brand" />
          <span className="font-medium">Indexing</span>
          <span className="hidden max-w-56 truncate text-muted-foreground lg:inline">{step}</span>
        </Link>
        <Stop label="Cancel indexing" pending={stopIndex.isPending} onClick={() => stopIndex.mutate()} />
      </Pill>
    );
  if (aws.data?.run?.running)
    return (
      <Pill>
        <Loader2 className="size-3.5 shrink-0 animate-spin text-brand" />
        <span className="font-medium">Waiting for AWS sign-in</span>
        <Stop label="Cancel sign-in" pending={stopSignIn.isPending} onClick={() => stopSignIn.mutate()} />
      </Pill>
    );
  if (busy > 0)
    return (
      <Pill>
        <Loader2 className="size-3.5 shrink-0 animate-spin text-muted-foreground" />
        <span className="text-muted-foreground">Working…</span>
      </Pill>
    );
  return null;
}

function Pill({ children }: { children: React.ReactNode }) {
  return <div className="flex h-8 max-w-sm items-center gap-2 rounded-full border bg-card px-3 text-xs shadow-sm">{children}</div>;
}

function Stop({ label, pending, onClick }: { label: string; pending: boolean; onClick: () => void }) {
  return (
    <button
      className={cn("-mr-1 flex items-center gap-1 rounded-full px-1.5 py-0.5 text-muted-foreground hover:bg-muted hover:text-foreground", pending && "opacity-50")}
      disabled={pending}
      onClick={onClick}
      aria-label={label}
      title={label}
    >
      <X className="size-3.5" /> Cancel
    </button>
  );
}
