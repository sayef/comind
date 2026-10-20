import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { AlertTriangle, LogIn } from "lucide-react";
import { useEffect, useRef } from "react";
import { Button } from "@/components/ui/button";
import { api } from "@/lib/api";
import { sentence } from "@/lib/utils";

/** For an S3 index: says when AWS credentials stopped working and offers the SSO sign-in. */
export function AwsBanner({ remote }: { remote: boolean }) {
  const qc = useQueryClient();
  const aws = useQuery({
    queryKey: ["aws"],
    queryFn: api.aws,
    enabled: remote,
    refetchInterval: (q) => (q.state.data?.run?.running ? 3000 : 5 * 60_000),
    refetchOnWindowFocus: true,
  });
  const login = useMutation({ mutationFn: api.awsLogin, onSuccess: () => qc.invalidateQueries({ queryKey: ["aws"] }) });
  const stop = useMutation({ mutationFn: api.cancelAwsLogin, onSuccess: () => qc.invalidateQueries({ queryKey: ["aws"] }) });
  const a = aws.data;
  const wasBad = useRef(false);
  useEffect(() => {
    if (a?.needed && a.ok === false) wasBad.current = true;
    if (a?.ok && wasBad.current) {
      wasBad.current = false;
      qc.invalidateQueries({ predicate: (q) => q.queryKey[0] !== "aws" });
    }
  }, [a, qc]);
  if (!a?.needed || a.ok) return null;
  const waiting = a.run?.running;

  return (
    <div className="flex flex-wrap items-center gap-3 border-b border-warning/40 bg-warning/10 px-4 py-2 text-sm md:px-6">
      <AlertTriangle className="size-4 shrink-0 text-warning" />
      <span className="min-w-0 flex-1 [overflow-wrap:anywhere]">
        {waiting
          ? `Waiting for you to approve the AWS sign-in in your browser${a.profile ? ` (profile ${a.profile})` : ""}…`
          : a.signin
            ? `The AWS sign-in for the S3 index has expired${a.profile ? ` (profile ${a.profile})` : ""}.`
            : sentence(a.message ?? "")}
        {!waiting && a.run?.ok === false && a.run.message && <span className="text-muted-foreground"> Last attempt: {sentence(a.run.message)}</span>}
      </span>
      {a.signin && waiting && (
        <Button size="sm" variant="ghost" loading={stop.isPending} onClick={() => stop.mutate()}>
          Cancel
        </Button>
      )}
      {a.signin && (
        <Button size="sm" variant="outline" disabled={waiting} loading={login.isPending || waiting} onClick={() => login.mutate()}>
          {!(login.isPending || waiting) && <LogIn />} {waiting ? "Waiting…" : "Sign in"}
        </Button>
      )}
    </div>
  );
}
