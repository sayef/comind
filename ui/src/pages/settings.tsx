import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { AlertTriangle, CheckCircle2 } from "lucide-react";
import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { ApiKey, TeamSection } from "@/components/team-settings";
import { api, type SettingsChange } from "@/lib/api";

const field = "flex flex-col gap-1 text-xs font-medium";
const select = "h-9 rounded-md border bg-card px-2 text-sm";

function Status({ ok, message }: { ok: boolean; message: string }) {
  return (
    <p className={`flex items-start gap-1.5 text-xs ${ok ? "text-muted-foreground" : "text-warning"}`}>
      {ok ? <CheckCircle2 className="mt-px size-3.5 shrink-0 text-brand" /> : <AlertTriangle className="mt-px size-3.5 shrink-0" />}
      {message}
    </p>
  );
}

function useSave() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (c: SettingsChange) => api.saveSettings(c),
    onSuccess: () => qc.invalidateQueries(),
  });
}

function Section({ title, description, children }: { title: string; description?: string; children: React.ReactNode }) {
  return (
    <Card>
      <CardHeader>
        <CardTitle>{title}</CardTitle>
        {description && <CardDescription>{description}</CardDescription>}
      </CardHeader>
      <CardContent className="flex flex-col gap-3">{children}</CardContent>
    </Card>
  );
}

export function SettingsPage() {
  const settings = useQuery({ queryKey: ["settings"], queryFn: api.settings });
  const team = useQuery({ queryKey: ["team"], queryFn: api.team });
  const teamLlm = !!(team.data?.settings.llm_model || team.data?.settings.llm_base_url);
  const s = settings.data;
  const saveIndex = useSave();
  const saveAws = useSave();
  const saveYou = useSave();
  const saveLlm = useSave();
  const saveFresh = useSave();
  const test = useMutation({ mutationFn: (dir: string) => api.testIndex(dir) });

  const [index, setIndex] = useState("");
  const [profile, setProfile] = useState("");
  const [name, setName] = useState("");
  const [names, setNames] = useState(true);
  const [base, setBase] = useState("");
  const [model, setModel] = useState("");
  const [ttl, setTtl] = useState(300);
  const [ssoAuto, setSsoAuto] = useState(false);
  useEffect(() => {
    if (!s) return;
    setIndex(s.index_dir ?? s.index_effective);
    setProfile(s.aws_profile ?? "");
    setName(s.author_name ?? "");
    setNames(s.record_names);
    setBase(s.llm_base_url ?? "");
    setModel(s.llm_model);
    setTtl(s.cache_ttl_secs);
    setSsoAuto(s.sso_auto_login);
  }, [s]);

  if (settings.isLoading || !s) return <Skeleton className="h-96 rounded-xl" />;
  const result = (m: typeof saveIndex) =>
    m.error ? <Status ok={false} message={m.error.message} /> : m.data ? <Status ok message={m.data.message} /> : null;

  return (
    <div className="flex max-w-3xl flex-col gap-5">
      <div>
        <h1 className="text-2xl font-semibold tracking-tight">Settings</h1>
        <p className="mt-1 text-sm text-muted-foreground">
          Saved in <span className="font-mono">{s.config_path}</span>, shared with the command line, the MCP server and hooks.
        </p>
      </div>

      <Section title="Index" description="Where the index lives: a local folder or an S3 location (s3://bucket/prefix).">
        <label className={field}>
          Location
          <Input value={index} onChange={(e) => setIndex(e.target.value)} placeholder="~/comind-index or s3://bucket/comind" />
        </label>
        <div className="flex flex-wrap gap-2">
          <Button variant="outline" size="sm" disabled={!index.trim() || test.isPending} onClick={() => test.mutate(index)}>
            Test
          </Button>
          <Button variant="brand" size="sm" disabled={!index.trim() || saveIndex.isPending} onClick={() => saveIndex.mutate({ index_dir: index })}>
            Use this index
          </Button>
        </div>
        {test.error && <Status ok={false} message={test.error.message} />}
        {test.data && <Status ok message={test.data.message} />}
        {result(saveIndex)}
      </Section>

      <TeamSection />

      <Section
        title="Storage access"
        description="S3 and S3-compatible stores use your AWS configuration (~/.aws): keys, SSO, credential_process or a custom endpoint. comind only remembers which profile to use."
      >
        <label className={field}>
          AWS profile
          <select className={select} value={profile} onChange={(e) => setProfile(e.target.value)}>
            <option value="">AWS default (environment or [default])</option>
            {s.aws_profiles.map((p) => (
              <option key={p} value={p}>
                {p}
              </option>
            ))}
          </select>
        </label>
        {s.aws_profile_env && s.aws_profile_env !== (s.aws_profile ?? "") && (
          <p className="text-xs text-muted-foreground">
            AWS_PROFILE=<span className="font-mono">{s.aws_profile_env}</span> is set in this process's environment and takes precedence.
          </p>
        )}
        {s.aws_status && <Status ok={s.aws_status.ok} message={s.aws_status.message} />}
        <label className="flex items-center gap-2 text-sm">
          <input type="checkbox" checked={ssoAuto} onChange={(e) => setSsoAuto(e.target.checked)} />
          When the SSO session expires, start the sign-in automatically (you still approve it in the browser)
        </label>
        <div>
          <Button variant="outline" size="sm" disabled={saveAws.isPending} onClick={() => saveAws.mutate({ aws_profile: profile, sso_auto_login: ssoAuto })}>
            Save
          </Button>
        </div>
        {result(saveAws)}
      </Section>

      <Section title="You" description="Shown on the ledger records you write and review.">
        <label className={field}>
          Name
          <Input value={name} onChange={(e) => setName(e.target.value)} placeholder={s.author_effective ?? "from git user.name"} />
        </label>
        <label className="flex items-center gap-2 text-sm">
          <input type="checkbox" checked={names} onChange={(e) => setNames(e.target.checked)} />
          Put my name on new records (off: only a pseudonymous id)
        </label>
        <div>
          <Button variant="outline" size="sm" disabled={saveYou.isPending} onClick={() => saveYou.mutate({ author_name: name, record_names: names })}>
            Save
          </Button>
        </div>
        {result(saveYou)}
      </Section>

      {teamLlm ? (
        <Card>
          <CardContent className="pt-6 text-sm text-muted-foreground">
            LLM: the team's settings apply (model {team.data?.settings.llm_model ?? s.defaults.llm_model}), so everyone gets the same
            results with their own key.
            <ApiKey />
          </CardContent>
        </Card>
      ) : (
      <Section
        title="Your LLM"
        description="Optional: summaries, flow walkthroughs and style guides while indexing. Any OpenAI-compatible endpoint works; leave the endpoint empty for OpenAI."
      >
        <div className="grid gap-3 sm:grid-cols-2">
          <label className={field}>
            Endpoint
            <Input value={base} onChange={(e) => setBase(e.target.value)} placeholder={s.defaults.llm_base_url} />
          </label>
          <label className={field}>
            Model
            <Input value={model} onChange={(e) => setModel(e.target.value)} placeholder={s.defaults.llm_model} />
          </label>
        </div>
        <div>
          <Button variant="outline" size="sm" disabled={saveLlm.isPending} onClick={() => saveLlm.mutate({ llm_base_url: base, llm_model: model })}>
            Save
          </Button>
        </div>
        {result(saveLlm)}
        <ApiKey />
      </Section>
      )}

      <Section title="Freshness" description="How often the app, the MCP server and hooks check for a newer index version.">
        <label className={field}>
          Seconds between checks
          <Input type="number" min={30} className="max-w-40" value={ttl} onChange={(e) => setTtl(Number(e.target.value))} />
        </label>
        <div>
          <Button variant="outline" size="sm" disabled={saveFresh.isPending} onClick={() => saveFresh.mutate({ cache_ttl_secs: ttl })}>
            Save
          </Button>
        </div>
        {result(saveFresh)}
      </Section>
    </div>
  );
}
