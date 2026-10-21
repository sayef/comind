import { useMemo } from "react";
import { RepoDot } from "@/components/symbol";
import type { OrgMap } from "@/lib/api";
import { cn } from "@/lib/utils";

type Edge = OrgMap["edges"][number];
export type Direction = "uses" | "usedBy";

/** Repositories with what each uses and what uses it, and the groups that depend on each other in a circle. */
export function useOrg(map: OrgMap | undefined) {
  return useMemo(() => {
    const uses = new Map<string, Map<string, Edge[]>>();
    const usedBy = new Map<string, Map<string, Edge[]>>();
    const symbols = new Map<string, number>();
    for (const n of map?.nodes ?? []) {
      uses.set(n.id, new Map());
      usedBy.set(n.id, new Map());
      symbols.set(n.id, Number(/\((\d+)\)$/.exec(n.name)?.[1] ?? 0));
    }
    for (const e of map?.edges ?? []) {
      if (!uses.has(e.src) || !uses.has(e.dst)) continue;
      uses.get(e.src)!.set(e.dst, [...(uses.get(e.src)!.get(e.dst) ?? []), e]);
      usedBy.get(e.dst)!.set(e.src, [...(usedBy.get(e.dst)!.get(e.src) ?? []), e]);
    }
    // Strongly connected components (Tarjan): every group of two or more is a cycle.
    let index = 0;
    const idx = new Map<string, number>();
    const low = new Map<string, number>();
    const stack: string[] = [];
    const onStack = new Set<string>();
    const cycles: string[][] = [];
    const strong = (v: string) => {
      idx.set(v, index);
      low.set(v, index++);
      stack.push(v);
      onStack.add(v);
      for (const w of uses.get(v)!.keys()) {
        if (!idx.has(w)) {
          strong(w);
          low.set(v, Math.min(low.get(v)!, low.get(w)!));
        } else if (onStack.has(w)) low.set(v, Math.min(low.get(v)!, idx.get(w)!));
      }
      if (low.get(v) === idx.get(v)) {
        const group: string[] = [];
        let w: string;
        do {
          w = stack.pop()!;
          onStack.delete(w);
          group.push(w);
        } while (w !== v);
        if (group.length > 1) cycles.push(group.sort());
      }
    };
    for (const v of [...uses.keys()].sort()) if (!idx.has(v)) strong(v);
    const repos = [...uses.keys()].sort();
    const entries = repos.filter((r) => usedBy.get(r)!.size === 0 && uses.get(r)!.size > 0);
    const foundations = repos.filter((r) => uses.get(r)!.size === 0 && usedBy.get(r)!.size > 0);
    return { repos, uses, usedBy, symbols, cycles, entries, foundations };
  }, [map]);
}

export type Org = ReturnType<typeof useOrg>;

interface Box {
  key: string;
  repo: string;
  path: string[];
  col: number;
  y: number;
  via: string;
  contract: boolean;
  kind: "open" | "closed" | "leaf" | "seen";
  more: number;
  loopTo: string | null;
}

const W = 216;
const H = 50;
const COLW = W + 64;
const ROWH = 58;

/** The visible tree: leaves on consecutive rows, each open box centred on its children. */
function layout(org: Org, roots: string[], dir: Direction, open: Set<string>) {
  const next = (r: string) => (dir === "uses" ? org.uses.get(r)! : org.usedBy.get(r)!);
  const boxes: Box[] = [];
  const seen = new Set<string>();
  let row = 0;
  const place = (repo: string, path: string[], via: Edge[] | null): number => {
    const key = [...path, repo].join("/");
    const kids = [...next(repo).entries()].sort(([a], [b]) => a.localeCompare(b));
    const first = !seen.has(repo);
    seen.add(repo);
    const children = kids.filter(([k]) => !path.includes(k) && k !== repo);
    const expanded = first && open.has(key) && children.length > 0;
    const names = [...new Set((via ?? []).flatMap((e) => e.via ?? []))];
    const box: Box = {
      key,
      repo,
      path,
      col: path.length,
      y: 0,
      via: via === null ? "" : names.length ? names[0] + (names.length > 1 ? ` (+${names.length - 1} more)` : "") : "code dependency",
      contract: names.length > 0,
      kind: !first ? "seen" : expanded ? "open" : children.length ? "closed" : "leaf",
      more: children.length,
      loopTo: kids.find(([k]) => path.includes(k))?.[0] ?? null,
    };
    boxes.push(box);
    if (!expanded) {
      box.y = row++ * ROWH;
      return box.y;
    }
    const ys = children.map(([k, es]) => place(k, [...path, repo], es));
    box.y = (ys[0] + ys[ys.length - 1]) / 2;
    return box.y;
  };
  for (const r of roots) {
    place(r, [], null);
    row += 0.4;
  }
  return boxes;
}

/** A left-to-right tree of what repositories use (or what uses them), opened box by box; a branch that
 * comes back to a repository it passed through gets an amber loop to it. */
export function TreeView({
  org,
  roots,
  dir,
  open,
  onToggle,
  selected,
}: {
  org: Org;
  roots: string[];
  dir: Direction;
  open: Set<string>;
  onToggle: (key: string) => void;
  selected: string | null;
}) {
  const boxes = useMemo(() => layout(org, roots, dir, open), [org, roots, dir, open]);
  const byKey = new Map(boxes.map((b) => [b.key, b]));
  const lit = new Set<string>();
  if (selected) {
    const parts = selected.split("/");
    for (let i = 1; i <= parts.length; i++) lit.add(parts.slice(0, i).join("/"));
  }
  const width = Math.max(0, ...boxes.map((b) => b.col)) * COLW + W + 8;
  const height = Math.max(0, ...boxes.map((b) => b.y)) + H + 8;
  const x = (b: Box) => b.col * COLW;

  const links: { d: string; on: boolean }[] = [];
  const loops: string[] = [];
  for (const b of boxes) {
    const parent = byKey.get(b.path.join("/"));
    if (parent && b.path.length) {
      const y1 = parent.y + H / 2;
      const y2 = b.y + H / 2;
      const mid = x(b) - 28;
      links.push({ d: `M${x(parent) + W},${y1} H${mid} V${y2} H${x(b)}`, on: lit.has(b.key) });
    }
    if (b.loopTo && b.kind !== "seen") {
      const target = boxes.find((t) => t.repo === b.loopTo && b.key.startsWith(`${t.key}/`));
      if (target) {
        // Out of the box's left side, along the gap between the columns, into the earlier box's right side.
        const gap = x(b) - 12;
        const y1 = b.y + H * 0.72;
        const y2 = target.y + H * 0.78;
        loops.push(`M${x(b)},${y1} H${gap} V${y2} H${x(target) + W + 6}`);
      }
    }
  }

  return (
    <div className="overflow-auto">
      <div className="relative m-5" style={{ width, height }}>
        <svg className="pointer-events-none absolute inset-0 overflow-visible" width={width} height={height} aria-hidden="true">
          <defs>
            <marker id="loop-tip" viewBox="0 0 8 8" refX="4" refY="4" markerWidth="8" markerHeight="8" orient="auto">
              <path d="M0,0 L8,4 L0,8 z" fill="var(--color-warning)" />
            </marker>
          </defs>
          {links
            .sort((a, b) => Number(a.on) - Number(b.on))
            .map((l, i) => (
              <path
                key={i}
                d={l.d}
                fill="none"
                stroke={l.on ? "var(--color-brand)" : "var(--color-input)"}
                strokeWidth={l.on ? 2 : 1.5}
                strokeLinejoin="round"
              />
            ))}
        </svg>
        {boxes.map((b) => (
          <button
            key={b.key}
            onClick={() => onToggle(b.key)}
            disabled={b.kind === "seen"}
            title={b.kind === "seen" ? `${b.repo} is shown elsewhere in this tree` : undefined}
            className={cn(
              "absolute flex flex-col justify-center gap-1 rounded-lg border bg-popover px-3 text-left transition-colors",
              b.kind === "seen" ? "border-dashed border-input text-muted-foreground" : "border-input hover:border-muted-foreground/60",
              b.loopTo && b.kind !== "seen" && "border-warning/70",
              lit.has(b.key) && "border-brand shadow-[0_0_0_4px_color-mix(in_srgb,var(--color-brand)_14%,transparent)]",
            )}
            style={{ left: x(b), top: b.y, width: W, height: H }}
          >
            <span className="flex w-full items-center gap-2 text-[13px]">
              <RepoDot repo={b.repo} className={b.kind === "seen" ? "opacity-50" : undefined} />
              <span className={cn("min-w-0 flex-1 truncate", (b.col === 0 || lit.has(b.key)) && "font-medium")}>{b.repo}</span>
              {b.loopTo && b.kind !== "seen" && (
                <span
                  className="font-mono text-[11px] text-warning"
                  title={`${b.repo} ${dir === "uses" ? "uses" : "is used by"} ${b.loopTo} too: a cycle`}
                >
                  ↺ {b.loopTo}
                </span>
              )}
              {b.kind === "closed" && <span className="font-mono text-[11px] text-muted-foreground">+{b.more}</span>}
              {b.kind === "open" && <span className="font-mono text-[11px] text-muted-foreground">−</span>}
            </span>
            <span
              className={cn(
                "truncate font-mono text-[10.5px]",
                b.kind !== "seen" && b.contract ? "text-accent-foreground" : "text-muted-foreground",
              )}
            >
              {b.col === 0 ? `${org.symbols.get(b.repo) ?? 0} symbols` : b.kind === "seen" ? `${b.via} · shown elsewhere` : b.via}
            </span>
          </button>
        ))}
        <svg className="pointer-events-none absolute inset-0 overflow-visible" width={width} height={height} aria-hidden="true">
          {loops.map((d, i) => (
            <path
              key={i}
              d={d}
              fill="none"
              stroke="var(--color-warning)"
              strokeWidth={1.5}
              strokeDasharray="5 4"
              strokeLinejoin="round"
              markerEnd="url(#loop-tip)"
            />
          ))}
        </svg>
      </div>
    </div>
  );
}
