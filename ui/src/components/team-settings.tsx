import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { KeyRound, Users } from "lucide-react";
import { useEffect, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Outcome } from "@/components/notice";
import { api, type KeySource, type TeamSettings } from "@/lib/api";

const field = "flex flex-col gap-1 text-xs font-medium";

const sources: Record<KeySource, string> = {
  team: "from your credentials file, for this team",
  default: "from your credentials file",
  environment: "from OPENAI_API_KEY in the environment",
  none: "not set",
};

/** Your own LLM API key, kept in your credentials file on this machine and never in the index. */
export function ApiKey() {
  const qc = useQueryClient();
  const team = useQuery({ queryKey: ["team"], queryFn: api.team, retry: false });
  const settings = useQuery({ queryKey: ["settings"], queryFn: api.settings });
  const source = team.data?.key_source ?? settings.data?.llm_key ?? "none";
  const name = team.data?.settings.name;
  const [key, setKey] = useState("");
  const save = useMutation({
    mutationFn: () => api.saveKey(key),
    onSuccess: () => (setKey(""), qc.invalidateQueries({ queryKey: ["team"] }), qc.invalidateQueries({ queryKey: ["settings"] })),
  });
  return (
    <div className="flex flex-col gap-2 border-t pt-3">
      <p className="flex items-center gap-2 text-sm font-medium">
        <KeyRound className="size-4" /> Your API key
        <Badge variant={source === "none" ? "outline" : "brand"}>{sources[source]}</Badge>
      </p>
      <p className="text-xs text-muted-foreground">
        Saved in ~/.config/comindly/credentials{name ? ` under ${name}` : ""}, readable only by you; otherwise OPENAI_API_KEY is used.
      </p>
      <div className="flex flex-wrap gap-2">
        <Input type="password" className="max-w-sm" value={key} onChange={(e) => setKey(e.target.value)} placeholder="Paste your API key" />
        <Button variant="outline" size="sm" disabled={!key.trim() || save.isPending} onClick={() => save.mutate()}>
          Save key
        </Button>
      </div>
      <Outcome error={save.error} message={save.data?.message} />
    </div>
  );
}

export function TeamSection() {
  const qc = useQueryClient();
  const team = useQuery({ queryKey: ["team"], queryFn: api.team });
  const t = team.data;
  const d = t?.defaults;
  const [form, setForm] = useState<TeamSettings>({});
  useEffect(() => {
    if (t) setForm(t.settings);
  }, [t]);
  const refresh = () => {
    qc.invalidateQueries({ queryKey: ["team"] });
    qc.invalidateQueries({ queryKey: ["settings"] });
  };
  const save = useMutation({ mutationFn: () => api.saveTeam(form, t?.version ?? null), onSuccess: refresh });
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
          Stored with the index and used by everyone on it: the same LLM and embedding model for all; each person brings their own API key. Empty fields use comindly's
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
        <Outcome error={save.error} message={save.data?.message} />

      </CardContent>
    </Card>
  );
}
