import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { AlertTriangle, CheckCircle2, FolderOpen, FolderSearch, GitBranch, GitFork, Loader2, Play, Plus, Trash2 } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { RepoDot } from "@/components/symbol";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog, DialogContent, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { Outcome } from "@/components/notice";
import { ApiError, api, type NeedsToken, pickFolder, type ScanHit } from "@/lib/api";

const field = "flex flex-col gap-1 text-xs font-medium";

/** A path input with a native folder picker in the desktop app. */
function FolderInput({ value, onChange, placeholder }: { value: string; onChange: (v: string) => void; placeholder: string }) {
  return (
    <div className="flex gap-2">
      <Input value={value} onChange={(e) => onChange(e.target.value)} placeholder={placeholder} className="font-mono text-xs" />
      {pickFolder && (
        <Button
          variant="outline"
          onClick={async () => {
            const p = await pickFolder!();
            if (p) onChange(p);
          }}
        >
          <FolderOpen /> Choose…
        </Button>
      )}
    </div>
  );
}

function Message({ error, data }: { error: Error | null; data?: { message: string } }) {
  return <Outcome error={error} message={data?.message} />;
}

function AddDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (o: boolean) => void }) {
  const qc = useQueryClient();
  const [path, setPath] = useState("");
  const add = useMutation({
    mutationFn: () => api.changeRepos({ add: [path] }),
    onSuccess: () => (qc.invalidateQueries({ queryKey: ["repos"] }), setPath(""), onOpenChange(false)),
  });
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="p-5">
        <DialogTitle className="text-base font-semibold">Add a repository</DialogTitle>
        <p className="mt-1 text-sm text-muted-foreground">Your local clone. comind indexes its default branch from its own checkout.</p>
        <div className="mt-4 flex flex-col gap-3">
          <label className={field}>
            Folder
            <FolderInput value={path} onChange={setPath} placeholder="~/dev/my-service" />
          </label>
          <Message error={add.error} />
          <div className="flex justify-end gap-2">
            <Button variant="ghost" onClick={() => onOpenChange(false)}>
              Cancel
            </Button>
            <Button variant="brand" disabled={!path.trim() || add.isPending} onClick={() => add.mutate()}>
              Add
            </Button>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}

function ScanDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (o: boolean) => void }) {
  const qc = useQueryClient();
  const [dir, setDir] = useState("");
  const [found, setFound] = useState<ScanHit[] | null>(null);
  const [checked, setChecked] = useState<Set<string>>(new Set());
  const [filter, setFilter] = useState("");
  const scan = useMutation({
    mutationFn: () => api.scanRepos(dir),
    onSuccess: (r) => (setFound(r.found), setChecked(new Set(r.found.filter((f) => f.listed).map((f) => f.path)))),
  });
  const apply = useMutation({
    mutationFn: () =>
      api.changeRepos({
        add: (found ?? []).filter((f) => checked.has(f.path) && !f.listed).map((f) => f.path),
        remove: (found ?? []).filter((f) => !checked.has(f.path) && f.listed).map((f) => f.path),
      }),
    onSuccess: () => (qc.invalidateQueries({ queryKey: ["repos"] }), onOpenChange(false), setFound(null)),
  });
  const shown = (found ?? []).filter((f) => !filter || f.name.toLowerCase().includes(filter.toLowerCase()));
  const toggle = (p: string) =>
    setChecked((c) => {
      const n = new Set(c);
      if (n.has(p)) n.delete(p);
      else n.add(p);
      return n;
    });
  const adds = (found ?? []).filter((f) => checked.has(f.path) && !f.listed).length;
  const drops = (found ?? []).filter((f) => !checked.has(f.path) && f.listed).length;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="top-[8%] max-h-[84vh] overflow-y-auto p-5">
        <DialogTitle className="text-base font-semibold">Scan a folder for repositories</DialogTitle>
        <p className="mt-1 text-sm text-muted-foreground">Tick the ones comind should index; untick to drop one from the list.</p>
        <div className="mt-4 flex flex-col gap-3">
          <label className={field}>
            Folder
            <FolderInput value={dir} onChange={setDir} placeholder="~/dev" />
          </label>
          <div>
            <Button variant="outline" size="sm" disabled={!dir.trim() || scan.isPending} onClick={() => scan.mutate()}>
              <FolderSearch /> Scan
            </Button>
          </div>
          <Message error={scan.error} />
          {found && (
            <>
              <div className="flex items-center gap-2">
                <Input value={filter} onChange={(e) => setFilter(e.target.value)} placeholder={`Filter ${found.length} repositories`} className="h-8 text-xs" />
                <Button variant="ghost" size="sm" onClick={() => setChecked(new Set([...checked, ...shown.map((f) => f.path)]))}>
                  All
                </Button>
                <Button variant="ghost" size="sm" onClick={() => setChecked(new Set([...checked].filter((p) => !shown.some((f) => f.path === p))))}>
                  None
                </Button>
              </div>
              <div className="max-h-72 overflow-y-auto rounded-md border">
                {shown.length === 0 && <p className="p-3 text-xs text-muted-foreground">No git repositories found.</p>}
                {shown.map((f) => (
                  <label key={f.path} className="flex cursor-pointer items-center gap-2.5 border-b px-3 py-2 text-sm last:border-0 hover:bg-muted/50">
                    <input type="checkbox" checked={checked.has(f.path)} onChange={() => toggle(f.path)} />
                    <span className="font-medium">{f.name}</span>
                    {f.listed && <Badge variant="outline">listed</Badge>}
                    <span className="ml-auto truncate font-mono text-[11px] text-muted-foreground">{f.remote ?? f.path}</span>
                  </label>
                ))}
              </div>
              <Message error={apply.error} />
              <div className="flex items-center justify-end gap-2">
                <span className="mr-auto text-xs text-muted-foreground">
                  {adds} to add, {drops} to remove
                </span>
                <Button variant="ghost" onClick={() => onOpenChange(false)}>
                  Cancel
                </Button>
                <Button variant="brand" disabled={adds + drops === 0 || apply.isPending} onClick={() => apply.mutate()}>
                  Apply
                </Button>
              </div>
            </>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}

function CloneDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (o: boolean) => void }) {
  const qc = useQueryClient();
  const [url, setUrl] = useState("");
  const [dir, setDir] = useState("");
  const [username, setUsername] = useState("");
  const [token, setToken] = useState("");
  const [asked, setAsked] = useState<NeedsToken | null>(null);
  const clone = useMutation({
    mutationFn: () => api.cloneRepo(url, dir, asked && token.trim() ? { username, token } : undefined),
    onSuccess: () => (qc.invalidateQueries({ queryKey: ["repos"] }), setUrl(""), setToken(""), setAsked(null), onOpenChange(false)),
    onError: (e) => {
      const need = e instanceof ApiError ? (e.body.needs_token as NeedsToken | undefined) : undefined;
      if (need) (setAsked(need), setUsername((u) => u || need.username));
    },
  });
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="p-5">
        <DialogTitle className="text-base font-semibold">Clone a repository</DialogTitle>
        <p className="mt-1 text-sm text-muted-foreground">
          HTTPS or SSH. comind uses the git access this machine already has: your SSH key, your git credential helper, or
          GITLAB_TOKEN / GITHUB_TOKEN. If none works it asks for a token.
        </p>
        <div className="mt-4 flex flex-col gap-3">
          <label className={field}>
            Clone URL
            <Input
              value={url}
              onChange={(e) => (setUrl(e.target.value), setAsked(null))}
              placeholder="https://gitlab.com/group/service.git"
              className="font-mono text-xs"
            />
          </label>
          <label className={field}>
            Into folder
            <FolderInput value={dir} onChange={setDir} placeholder="~/dev" />
          </label>
          {asked && (
            <div className="flex flex-col gap-2 rounded-lg border bg-muted/40 p-3">
              <p className="text-xs text-muted-foreground">
                {asked.host} needs an access token with read access to the repository. Your git credential helper (on macOS the
                keychain) keeps it after a successful clone.
              </p>
              <div className="grid gap-2 sm:grid-cols-[10rem_1fr]">
                <Input value={username} onChange={(e) => setUsername(e.target.value)} placeholder="Username" className="text-xs" />
                <Input type="password" value={token} onChange={(e) => setToken(e.target.value)} placeholder="Access token" autoFocus />
              </div>
            </div>
          )}
          {!asked && <Message error={clone.error} />}
          {asked && clone.error && token && <Message error={clone.error} />}
          <div className="flex justify-end gap-2">
            <Button variant="ghost" onClick={() => onOpenChange(false)}>
              Cancel
            </Button>
            <Button
              variant="brand"
              disabled={!url.trim() || !dir.trim() || (!!asked && !token.trim()) || clone.isPending}
              onClick={() => clone.mutate()}
            >
              {clone.isPending && <Loader2 className="animate-spin" />} {asked ? "Clone with token" : "Clone and add"}
            </Button>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}

function IndexPanel() {
  const qc = useQueryClient();
  const [llm, setLlm] = useState(false);
  const [full, setFull] = useState(false);
  const job = useQuery({ queryKey: ["index-job"], queryFn: api.indexJob, refetchInterval: (q) => (q.state.data?.running ? 1000 : false) });
  const start = useMutation({ mutationFn: () => api.startIndex(llm), onSuccess: () => qc.invalidateQueries({ queryKey: ["index-job"] }) });
  const j = job.data;
  const [seen, setSeen] = useState<string | null>(null);
  useEffect(() => {
    if (j && !j.running && j.ok && j.finished && j.finished !== seen) {
      setSeen(j.finished);
      qc.invalidateQueries({ predicate: (q) => q.queryKey[0] !== "index-job" });
    }
  }, [j, seen, qc]);
  const watch = useQuery({ queryKey: ["index-watch"], queryFn: api.indexWatch, refetchInterval: 30_000 });
  const settings = useQuery({ queryKey: ["settings"], queryFn: api.settings });
  const auto = useMutation({
    mutationFn: (on: boolean) => api.saveSettings({ auto_index: on }),
    onSuccess: () => (qc.invalidateQueries({ queryKey: ["settings"] }), qc.invalidateQueries({ queryKey: ["index-watch"] })),
  });
  const lines = j?.lines ?? [];
  const shown = full ? lines : lines.slice(-12);

  return (
    <Card>
      <CardHeader className="flex-row items-start justify-between gap-4">
        <div>
          <CardTitle>Index</CardTitle>
          <CardDescription>
            Fetches each repository's default branch into comind's own checkout, then rebuilds the index. Unchanged code reuses its
            embeddings{llm ? " and summaries" : ""}.
          </CardDescription>
        </div>
        <div className="flex shrink-0 flex-col items-end gap-2">
          <Button variant="brand" disabled={j?.running || start.isPending} onClick={() => start.mutate()}>
            {j?.running ? <Loader2 className="animate-spin" /> : <Play />} {j?.running ? "Indexing…" : "Index now"}
          </Button>
          <label className="flex items-center gap-1.5 text-xs text-muted-foreground">
            <input type="checkbox" checked={llm} onChange={(e) => setLlm(e.target.checked)} />
            LLM summaries (sends code to the LLM)
          </label>
        </div>
      </CardHeader>
      <CardContent className="flex flex-col gap-2">
        <label className="flex items-center gap-2 text-sm">
          <input
            type="checkbox"
            checked={settings.data?.auto_index ?? false}
            disabled={!settings.data || auto.isPending}
            onChange={(e) => auto.mutate(e.target.checked)}
          />
          Re-index automatically when a default branch moves (checked every {settings.data?.cache_ttl_secs ?? 300} s, without LLM)
        </label>
        {watch.data?.enabled && watch.data.last.checked && (
          <p className="text-xs text-muted-foreground">
            Last check {new Date(watch.data.last.checked).toLocaleTimeString()}: {watch.data.last.result}
          </p>
        )}
        <Message error={start.error} />
        {j && !j.running && j.ok !== null && (
          <p className={`flex items-center gap-1.5 text-sm ${j.ok ? "" : "text-warning"}`}>
            {j.ok ? <CheckCircle2 className="size-4 text-brand" /> : <AlertTriangle className="size-4" />}
            {j.ok ? "Indexed; every page shows the new index." : "Indexing failed; see the log."}
            {j.finished && <span className="text-xs text-muted-foreground">{new Date(j.finished).toLocaleString()}</span>}
          </p>
        )}
        {lines.length > 0 && (
          <>
            <pre className="max-h-80 overflow-auto rounded-lg border bg-muted/40 p-3 font-mono text-[11px] leading-relaxed">{shown.join("\n")}</pre>
            {lines.length > 12 && (
              <button className="self-start text-xs text-muted-foreground hover:text-foreground" onClick={() => setFull((f) => !f)}>
                {full ? "Show less" : `Show all ${lines.length} lines`}
              </button>
            )}
          </>
        )}
      </CardContent>
    </Card>
  );
}

export function RepositoriesPage() {
  const qc = useQueryClient();
  const repos = useQuery({ queryKey: ["repos"], queryFn: api.repos });
  const [dialog, setDialog] = useState<"add" | "scan" | "clone" | null>(null);
  const remove = useMutation({ mutationFn: (path: string) => api.changeRepos({ remove: [path] }), onSuccess: () => qc.invalidateQueries({ queryKey: ["repos"] }) });
  const remote = useMemo(() => repos.data?.root?.includes("://"), [repos.data]);
  const dropped = repos.data?.unlisted ?? [];
  const close = (o: boolean) => !o && setDialog(null);

  return (
    <div className="flex flex-col gap-5">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">Repositories</h1>
          <p className="mt-1 max-w-3xl text-sm text-muted-foreground">
            What comind indexes. Each repository is indexed at its default branch from comind's own checkout, so your folders and
            branches are never touched.
          </p>
        </div>
        <div className="flex flex-wrap gap-2">
          <Button variant="outline" onClick={() => setDialog("add")}>
            <Plus /> Add folder
          </Button>
          <Button variant="outline" onClick={() => setDialog("scan")}>
            <FolderSearch /> Scan a folder
          </Button>
          <Button variant="outline" onClick={() => setDialog("clone")}>
            <GitFork /> Clone
          </Button>
        </div>
      </div>

      {dropped.length > 0 ? (
        <p className="flex items-start gap-2 rounded-lg border border-warning/40 bg-warning/5 p-3 text-sm">
          <AlertTriangle className="mt-0.5 size-4 shrink-0 text-warning" />
          <span className="min-w-0 [overflow-wrap:anywhere]">
            Also in this index but not in your list: {dropped.join(", ")}. They were indexed elsewhere (another machine or CI);
            indexing from here would drop them. Add them here first, or change the list where they are indexed.
          </span>
        </p>
      ) : (
        remote && (
          <p className="text-xs text-muted-foreground">Shared in S3: indexing from here publishes a new version for everyone using it.</p>
        )
      )}

      <Card className="overflow-hidden">
        {repos.isLoading && <Skeleton className="h-32" />}
        {repos.data && repos.data.repos.length === 0 && (
          <p className="p-6 text-center text-sm text-muted-foreground">No repositories yet. Add a folder, scan one, or clone a repository.</p>
        )}
        {repos.data?.repos.map((r) => (
          <div key={r.path} className="flex flex-wrap items-center gap-x-4 gap-y-1 border-b px-4 py-3 last:border-0">
            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-2">
                <RepoDot repo={r.name} />
                <span className="font-medium">{r.name}</span>
                {!r.exists && <Badge variant="danger">folder missing</Badge>}
                {r.symbols == null ? <Badge variant="outline">not indexed yet</Badge> : <Badge variant="outline">{r.symbols.toLocaleString()} symbols</Badge>}
              </div>
              <div className="mt-0.5 truncate font-mono text-[11px] text-muted-foreground">{r.path}</div>
            </div>
            <div className="flex items-center gap-1.5 text-xs text-muted-foreground" title="What comind indexes">
              <GitBranch className="size-3.5" /> indexes <span className="font-mono text-foreground">origin/{r.branch ?? "?"}</span>
              {r.indexed_commit && <span className="font-mono">@ {r.indexed_commit.slice(0, 7)}</span>}
            </div>
            <div className="text-xs text-muted-foreground" title="Your own checkout, left as it is">
              yours: <span className="font-mono">{r.your_branch ?? "?"}</span>
            </div>
            <Button variant="ghost" size="icon" className="size-8" aria-label="Remove" title="Remove from the list (your folder stays)" onClick={() => remove.mutate(r.path)}>
              <Trash2 />
            </Button>
          </div>
        ))}
      </Card>

      <IndexPanel />

      <AddDialog open={dialog === "add"} onOpenChange={close} />
      <ScanDialog open={dialog === "scan"} onOpenChange={close} />
      <CloneDialog open={dialog === "clone"} onOpenChange={close} />
    </div>
  );
}
