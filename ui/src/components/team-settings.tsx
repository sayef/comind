import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { AlertTriangle, Check, CheckCircle2, Copy, KeyRound, Lock, Users } from "lucide-react";
import { useEffect, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog, DialogContent, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { api, type TeamSettings } from "@/lib/api";

const field = "flex flex-col gap-1 text-xs font-medium";

function Note({ error, message }: { error?: Error | null; message?: string }) {
  if (error)
    return (
      <p className="flex items-start gap-1.5 text-xs text-warning">
        <AlertTriangle className="mt-px size-3.5 shrink-0" /> {error.message}
      </p>
    );
  return message ? (
    <p className="flex items-start gap-1.5 text-xs text-muted-foreground">
      <CheckCircle2 className="mt-px size-3.5 shrink-0 text-brand" /> {message}
    </p>
  ) : null;
}

/** Shows a new team password once, to copy into a password manager. */
function NewPassword({ password, onClose }: { password: string | null; onClose: () => void }) {
  const [copied, setCopied] = useState(false);
  return (
    <Dialog open={!!password} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="p-5">
        <DialogTitle className="text-base font-semibold">Team password</DialogTitle>
        <p className="mt-1 text-sm text-muted-foreground">
          Shown once. Copy it and share it with your team; they enter it when they join.
        </p>
        <div className="mt-4 flex items-center gap-2 rounded-lg border bg-muted/50 p-3">
          <code className="flex-1 break-all font-mono text-sm">{password}</code>
          <Button
            size="icon"
            variant="ghost"
            aria-label="Copy"
            onClick={() => (navigator.clipboard?.writeText(password ?? ""), setCopied(true))}
          >
            {copied ? <Check /> : <Copy />}
          </Button>
        </div>
        <div className="mt-4 flex justify-end">
          <Button variant="brand" onClick={onClose}>
            I have stored it
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}

export function TeamSection() {
  const qc = useQueryClient();
  const team = useQuery({ queryKey: ["team"], queryFn: api.team });
  const t = team.data;
  const d = t?.defaults;
  const [form, setForm] = useState<TeamSettings>({});
  const [key, setKey] = useState("");
  const [pw, setPw] = useState("");
  const [shown, setShown] = useState<string | null>(null);
  useEffect(() => {
    if (t) setForm(t.settings);
  }, [t]);
  const refresh = () => {
    qc.invalidateQueries({ queryKey: ["team"] });
    qc.invalidateQueries({ queryKey: ["settings"] });
  };
  const save = useMutation({ mutationFn: () => api.saveTeam(form, t?.version ?? null), onSuccess: refresh });
  const setTeamKey = useMutation({
    mutationFn: () => api.teamKey(key),
    onSuccess: (r) => (setKey(""), r.password && setShown(r.password), refresh()),
  });
  const unlock = useMutation({ mutationFn: () => api.teamUnlock(pw), onSuccess: () => (setPw(""), refresh()) });
  const set = (k: keyof TeamSettings, v: string | boolean | undefined) => setForm((f) => ({ ...f, [k]: v }));
  const steps: [keyof TeamSettings, string][] = [
    ["enrich", "Summaries"],
    ["flows", "Flow walkthroughs"],
    ["guide", "Style guide"],
  ];

  if (!t) return null;
  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <Users className="size-4" /> Team
        </CardTitle>
        <CardDescription>
          Stored with the index and used by everyone on it: the same LLM, key and embedding model for all. Empty fields use comind's
          default, shown in grey. Changing them needs write access to the index.
          {t.changed_by && (
            <span>
              {" "}
              Last changed by {t.changed_by}
              {t.changed_at ? `, ${new Date(t.changed_at).toLocaleString()}` : ""}.
            </span>
          )}
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        <div className="grid gap-3 sm:grid-cols-2">
          <label className={field}>
            Team name
            <Input value={form.name ?? ""} onChange={(e) => set("name", e.target.value)} placeholder="e.g. Platform team" />
          </label>
          <label className={field}>
            Embedding model
            <Input value={form.embed_model ?? ""} onChange={(e) => set("embed_model", e.target.value)} placeholder={d?.embed_model} />
          </label>
          <label className={field}>
            LLM endpoint
            <Input value={form.llm_base_url ?? ""} onChange={(e) => set("llm_base_url", e.target.value)} placeholder={d?.llm_base_url} />
          </label>
          <label className={field}>
            LLM model
            <Input value={form.llm_model ?? ""} onChange={(e) => set("llm_model", e.target.value)} placeholder={d?.llm_model} />
          </label>
        </div>
        <div className="flex flex-wrap gap-4 text-sm">
          {steps.map(([k, label]) => (
            <label key={k} className="flex items-center gap-2">
              <input type="checkbox" checked={form[k] !== false} onChange={(e) => set(k, e.target.checked)} />
              {label} when indexing
            </label>
          ))}
        </div>
        <div>
          <Button variant="outline" size="sm" disabled={save.isPending} onClick={() => save.mutate()}>
            Save for the team
          </Button>
        </div>
        <Note error={save.error} message={save.data?.message} />

        <div className="flex flex-col gap-2 border-t pt-4">
          <p className="flex items-center gap-2 text-sm font-medium">
            <KeyRound className="size-4" /> Team LLM key
            {!t.has_key && <Badge variant="outline">not set</Badge>}
            {t.has_key && t.unlocked && <Badge variant="brand">set, unlocked here</Badge>}
            {t.has_key && !t.unlocked && <Badge variant="warning">locked</Badge>}
          </p>
          <p className="text-xs text-muted-foreground">
            Encrypted with the team password (Argon2id + XChaCha20-Poly1305); it is never shown again once saved.
          </p>
          {t.has_key && !t.unlocked && (
            <div className="flex flex-wrap gap-2">
              <Input type="password" className="max-w-sm" value={pw} onChange={(e) => setPw(e.target.value)} placeholder="Team password" />
              <Button variant="brand" size="sm" disabled={!pw.trim() || unlock.isPending} onClick={() => unlock.mutate()}>
                <Lock /> Unlock
              </Button>
            </div>
          )}
          <Note error={unlock.error} />
          {(!t.has_key || t.unlocked) && (
            <div className="flex flex-wrap gap-2">
              <Input
                type="password"
                className="max-w-sm"
                value={key}
                onChange={(e) => setKey(e.target.value)}
                placeholder={t.has_key ? "Paste a new key to replace it" : "Paste the team's API key"}
              />
              <Button variant="outline" size="sm" disabled={!key.trim() || setTeamKey.isPending} onClick={() => setTeamKey.mutate()}>
                {t.has_key ? "Replace key" : "Save key"}
              </Button>
            </div>
          )}
          <Note error={setTeamKey.error} message={setTeamKey.data?.message} />
        </div>

      </CardContent>
      <NewPassword password={shown} onClose={() => setShown(null)} />
    </Card>
  );
}
