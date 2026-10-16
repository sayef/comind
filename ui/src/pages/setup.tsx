import { useQuery } from "@tanstack/react-query";
import { AlertTriangle, BookOpen, Check, CheckCircle2, CircleDashed, Copy } from "lucide-react";
import { useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { api } from "@/lib/api";

const DOCS = "https://comind.pages.dev";

/** An example question per MCP tool; agents pick the tool themselves. */
const ASK: Record<string, string> = {
  search: "Where do we retry outgoing HTTP calls?",
  find: "Find the OrderService class.",
  zoom: "Show me everything around OrderService.place_order.",
  thread: "Trace what happens after place_order is called.",
  flow: "Walk me through the checkout flow.",
  repos: "Which repositories are indexed and how do they depend on each other?",
  suggest: "What can I ask about the payments module?",
  ripple: "What breaks in other services if I change this function's signature?",
  context_pack: "What should I read before changing OrderService.place_order?",
  preflight: "I am about to change auth.py and errors.py. Anything I should know?",
  contracts: "Who calls GET /orders/{id}, and which queue does the order service publish to?",
  guide: "Which conventions should I follow in this repository?",
  why: "Why is this function written this way? Did anyone try something else?",
  remember: "Remember that the cache TTL of None means entries never expire.",
  review: "That constraint about pinning the client library no longer applies; archive it.",
};

function Code({ children }: { children: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <div className="group relative">
      <pre className="overflow-x-auto rounded-lg border bg-muted/50 p-3 pr-12 font-mono text-xs leading-relaxed">{children}</pre>
      <Button
        size="icon"
        variant="ghost"
        className="absolute right-1.5 top-1.5 size-7"
        aria-label="Copy"
        onClick={() => {
          navigator.clipboard?.writeText(children);
          setCopied(true);
          setTimeout(() => setCopied(false), 1500);
        }}
      >
        {copied ? <Check /> : <Copy />}
      </Button>
    </div>
  );
}

function Step({ n, title, children }: { n: number; title: string; children: React.ReactNode }) {
  return (
    <div className="flex gap-4">
      <div className="flex size-7 shrink-0 items-center justify-center rounded-full bg-accent text-sm font-semibold text-accent-foreground">{n}</div>
      <div className="min-w-0 flex-1">
        <h3 className="font-medium">{title}</h3>
        <div className="mt-2 flex flex-col gap-3 text-sm text-muted-foreground">{children}</div>
      </div>
    </div>
  );
}

const AGENTS = ["claude", "cursor", "codex", "vscode", "other"] as const;
type Agent = (typeof AGENTS)[number];
const AGENT_NAME: Record<Agent, string> = { claude: "Claude Code", cursor: "Cursor", codex: "Codex", vscode: "VS Code", other: "Other" };

function connect(agent: Agent, index: string, remote: boolean) {
  const args = ["serve", "--index-dir", index];
  const env = remote ? { AWS_PROFILE: "<profile>", AWS_REGION: "<region>" } : null;
  const json = (servers: string, extra: object = {}) =>
    JSON.stringify({ [servers]: { comind: { ...extra, command: "comind", args, ...(env ? { env } : {}) } } }, null, 2);
  switch (agent) {
    case "claude":
      return {
        where: "Run once in a terminal (user scope: every project):",
        code: `claude mcp add --scope user comind${env ? " -e AWS_PROFILE=<profile> -e AWS_REGION=<region>" : ""} -- comind ${args.join(" ")}`,
      };
    case "cursor":
      return { where: "Add to ~/.cursor/mcp.json:", code: json("mcpServers") };
    case "codex":
      return {
        where: "Add to ~/.codex/config.toml:",
        code: `[mcp_servers.comind]\ncommand = "comind"\nargs = ${JSON.stringify(args)}${env ? `\nenv = { AWS_PROFILE = "<profile>", AWS_REGION = "<region>" }` : ""}`,
      };
    case "vscode":
      return { where: "Add to .vscode/mcp.json in your workspace:", code: json("servers", { type: "stdio" }) };
    default:
      return { where: "Any MCP client that starts a server over stdio:", code: json("mcpServers") };
  }
}

export function SetupPage() {
  const setup = useQuery({ queryKey: ["setup"], queryFn: api.setup });
  const [agent, setAgent] = useState<Agent>("claude");
  const s = setup.data;
  const index = s?.index || "<index-dir>";
  const how = connect(agent, index, !!s?.remote);

  return (
    <div className="flex flex-col gap-6">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">Connect your agent</h1>
          <p className="mt-1 max-w-3xl text-sm text-muted-foreground">
            comind answers your coding agent's questions about the whole codebase: what calls what across repositories, which services
            talk through which routes and queues, what a change can break, and what the team recorded about the code.
          </p>
        </div>
        <Button variant="outline" asChild>
          <a href={DOCS} target="_blank" rel="noreferrer">
            <BookOpen /> Documentation
          </a>
        </Button>
      </div>

      <Card>
        <CardHeader>
          <CardTitle>This setup</CardTitle>
          <CardDescription>
            comind {s?.version ?? "…"} · index <span className="font-mono">{index}</span>
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-1.5 text-sm">
          {setup.isLoading && <Skeleton className="h-24" />}
          {s?.checks.map((c) => (
            <div key={c.message} className="flex items-start gap-2">
              {c.ok ? (
                <CheckCircle2 className="mt-0.5 size-4 shrink-0 text-brand" />
              ) : c.optional ? (
                <CircleDashed className="mt-0.5 size-4 shrink-0 text-muted-foreground" />
              ) : (
                <AlertTriangle className="mt-0.5 size-4 shrink-0 text-warning" />
              )}
              <span className={c.ok ? "" : "text-muted-foreground"}>{c.message}</span>
              {!c.ok && c.optional && <Badge variant="outline">optional</Badge>}
            </div>
          ))}
        </CardContent>
      </Card>

      <Card>
        <CardContent className="flex flex-col gap-6 pt-6">
          <Step n={1} title="Install comind on your machine">
            <p>Prebuilt for macOS and Linux. Check with <code className="font-mono">comind --version</code>.</p>
            <Code>{"curl -LsSf https://raw.githubusercontent.com/sayef/comind/main/scripts/install.sh | sh"}</Code>
          </Step>
          <Step n={2} title="Register the MCP server with your agent">
            <Tabs value={agent} onValueChange={(v) => setAgent(v as Agent)}>
              <TabsList>
                {AGENTS.map((a) => (
                  <TabsTrigger key={a} value={a}>
                    {AGENT_NAME[a]}
                  </TabsTrigger>
                ))}
              </TabsList>
            </Tabs>
            <p>{how.where}</p>
            <Code>{how.code}</Code>
            {s?.remote && (
              <p>
                The index is in S3, so the server needs AWS credentials: set the profile and region (or other AWS variables) your account
                uses. Hooks read a local mirror that refreshes itself.
              </p>
            )}
            <p>Restart the agent; it then lists comind's tools and calls them on its own when a question needs them.</p>
          </Step>
          <Step n={3} title="Optional: hooks for Claude Code">
            <p>
              Hooks put what the team recorded in front of the agent without it asking: records about this repository when a session
              starts, and about a file just before the agent edits it (each record once per session). Print the settings and merge them
              into <code className="font-mono">~/.claude/settings.json</code>:
            </p>
            <Code>{"comind init-hooks claude"}</Code>
            <p>
              For Cursor, Gemini or Codex: <code className="font-mono">comind init-hooks cursor</code> (or{" "}
              <code className="font-mono">gemini</code>, <code className="font-mono">codex</code>, <code className="font-mono">generic</code>).
            </p>
          </Step>
          <Step n={4} title="Check it">
            <Code>{`comind doctor --index-dir ${index}`}</Code>
          </Step>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>What your agent can ask</CardTitle>
          <CardDescription>Ask in plain words; the agent chooses the tool. Each tool, with a question that uses it:</CardDescription>
        </CardHeader>
        <CardContent className="grid gap-3 md:grid-cols-2">
          {setup.isLoading && <Skeleton className="h-40 md:col-span-2" />}
          {s?.tools.map((t) => (
            <div key={t.name} className="rounded-lg border p-3">
              <div className="flex items-center gap-2">
                <code className="font-mono text-sm font-semibold">{t.name}</code>
              </div>
              {ASK[t.name] && <p className="mt-1.5 text-sm">“{ASK[t.name]}”</p>}
              <p className="mt-1.5 line-clamp-3 text-xs text-muted-foreground" title={t.description}>
                {t.description}
              </p>
            </div>
          ))}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>From the terminal</CardTitle>
          <CardDescription>The same answers without an agent.</CardDescription>
        </CardHeader>
        <CardContent>
          <Code>
            {[
              `comind search "where do we retry HTTP calls" --index-dir ${index}`,
              `comind explore OrderService.place_order --index-dir ${index}   # neighbourhood, blast radius, context`,
              `comind contracts --dangling --index-dir ${index}             # routes or queues missing a side`,
              `comind why OrderService --index-dir ${index}                 # what the team recorded`,
              `comind preflight . --index-dir ${index}                      # before you commit`,
              `comind remember constraint --on OrderService --title "…" --index-dir ${index}`,
            ].join("\n")}
          </Code>
        </CardContent>
      </Card>
    </div>
  );
}
