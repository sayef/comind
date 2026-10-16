import { useQuery } from "@tanstack/react-query";
import { Command } from "cmdk";
import { FileCode2, FunctionSquare, Network, Shapes } from "lucide-react";
import { useEffect, useState } from "react";
import { useNavigate } from "react-router";
import { Dialog, DialogContent, DialogTitle } from "@/components/ui/dialog";
import { api, type Node } from "@/lib/api";
import { CONTRACTS } from "@/lib/utils";

function iconFor(n: Node) {
  if (n.repo === CONTRACTS) return Network;
  if (n.kind === "File") return FileCode2;
  if (["Class", "Struct", "Interface", "Trait", "Enum"].includes(n.kind)) return Shapes;
  return FunctionSquare;
}

export function CommandPalette({ open, onOpenChange }: { open: boolean; onOpenChange: (o: boolean) => void }) {
  const [q, setQ] = useState("");
  const [debounced, setDebounced] = useState("");
  const navigate = useNavigate();
  useEffect(() => {
    const t = setTimeout(() => setDebounced(q.trim()), 150);
    return () => clearTimeout(t);
  }, [q]);
  const results = useQuery({
    queryKey: ["find", debounced],
    queryFn: () => api.find(debounced),
    enabled: debounced.length > 1,
  });

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogTitle className="sr-only">Search the index</DialogTitle>
        <Command shouldFilter={false} className="flex flex-col">
          <Command.Input
            value={q}
            onValueChange={setQ}
            placeholder="Search symbols, files, contracts…"
            className="h-12 border-b bg-transparent px-4 text-sm outline-none placeholder:text-muted-foreground"
          />
          <Command.List className="max-h-[60vh] overflow-y-auto p-2">
            {debounced.length > 1 && !results.isFetching && (
              <Command.Empty className="px-3 py-8 text-center text-sm text-muted-foreground">No matches.</Command.Empty>
            )}
            {debounced.length <= 1 && (
              <div className="px-3 py-8 text-center text-sm text-muted-foreground">Type a name, path or route.</div>
            )}
            {results.data?.map((n) => {
              const Icon = iconFor(n);
              return (
                <Command.Item
                  key={n.id}
                  value={n.id}
                  onSelect={() => {
                    onOpenChange(false);
                    navigate(`/explorer?focus=${encodeURIComponent(n.id)}`);
                  }}
                  className="flex cursor-pointer items-center gap-3 rounded-md px-3 py-2 text-sm data-[selected=true]:bg-muted"
                >
                  <Icon className="size-4 shrink-0 text-muted-foreground" />
                  <div className="min-w-0 flex-1">
                    <div className="truncate font-medium">{n.name}</div>
                    <div className="truncate font-mono text-xs text-muted-foreground">
                      {n.repo === CONTRACTS ? "contract" : `${n.repo} · ${n.location}`}
                    </div>
                  </div>
                  <span className="text-xs text-muted-foreground">{n.kind}</span>
                </Command.Item>
              );
            })}
          </Command.List>
        </Command>
      </DialogContent>
    </Dialog>
  );
}
