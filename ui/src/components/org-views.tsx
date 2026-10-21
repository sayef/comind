import { ArrowLeft } from "lucide-react";
import { useLayoutEffect, useMemo, useRef, useState } from "react";
import { RepoDot } from "@/components/symbol";
import type { OrgMap } from "@/lib/api";
import { cn } from "@/lib/utils";

type Edge = OrgMap["edges"][number];

/** One repository: its symbol count, and what it uses and what uses it, per other repository. */
export interface Repo {
  id: string;
  symbols: number;
  uses: Map<string, Edge[]>;
  usedBy: Map<string, Edge[]>;
  layer: number;
}

/** Repositories with their links, in layers: entry points first, shared foundations last. */
export function useRepos(map: OrgMap | undefined) {
  return useMemo(() => {
    const repos = new Map<string, Repo>();
    if (!map) return { repos, layers: [] as Repo[][] };
    for (const n of map.nodes) {
      const symbols = Number(/\((\d+)\)$/.exec(n.name)?.[1] ?? 0);
      repos.set(n.id, { id: n.id, symbols, uses: new Map(), usedBy: new Map(), layer: 0 });
    }
    for (const e of map.edges) {
      const a = repos.get(e.src);
      const b = repos.get(e.dst);
      if (!a || !b) continue;
      a.uses.set(b.id, [...(a.uses.get(b.id) ?? []), e]);
      b.usedBy.set(a.id, [...(b.usedBy.get(a.id) ?? []), e]);
    }
    // Longest path from the entry points, ignoring the edges that close a cycle.
    const state = new Map<string, 0 | 1 | 2>();
    const back = new Set<string>();
    const visit = (id: string) => {
      state.set(id, 1);
      for (const to of repos.get(id)!.uses.keys()) {
        if (state.get(to) === 1) back.add(`${id}>${to}`);
        else if (!state.get(to)) visit(to);
      }
      state.set(id, 2);
    };
    for (const id of [...repos.keys()].sort()) if (!state.get(id)) visit(id);
    const order: string[] = [];
    const seen = new Set<string>();
    const topo = (id: string) => {
      seen.add(id);
      for (const to of repos.get(id)!.uses.keys()) if (!back.has(`${id}>${to}`) && !seen.has(to)) topo(to);
      order.push(id);
    };
    for (const id of [...repos.keys()].sort()) if (!seen.has(id)) topo(id);
    for (const id of order.reverse()) {
      const r = repos.get(id)!;
      for (const to of r.uses.keys()) {
        if (back.has(`${id}>${to}`)) continue;
        const t = repos.get(to)!;
        t.layer = Math.max(t.layer, r.layer + 1);
      }
    }
    // Foundations that use nothing themselves all sit in the bottom row.
    const deepest = Math.max(0, ...[...repos.values()].map((r) => r.layer));
    for (const r of repos.values()) if (r.uses.size === 0 && r.usedBy.size > 0) r.layer = deepest;
    const layers: Repo[][] = [];
    for (const r of repos.values()) (layers[r.layer] ??= []).push(r);
    // Order each column by where its neighbours sit, so lines cross less.
    const pos = new Map<string, number>();
    layers.forEach((col, li) => {
      if (li === 0) col.sort((a, b) => b.uses.size - a.uses.size || a.id.localeCompare(b.id));
      else {
        const centre = (r: Repo) => {
          const p = [...r.usedBy.keys()].map((k) => pos.get(k)).filter((x): x is number => x !== undefined);
          return p.length ? p.reduce((s, x) => s + x, 0) / p.length : Number.MAX_SAFE_INTEGER;
        };
        col.sort((a, b) => centre(a) - centre(b) || a.id.localeCompare(b.id));
      }
      col.forEach((r, i) => pos.set(r.id, i));
    });
    return { repos, layers: layers.filter(Boolean) };
  }, [map]);
}

const contract = (es: Edge[]) => es.some((e) => e.kind === "contract");
const count = (es: Edge[]) => es.reduce((s, e) => s + e.weight, 0);
const rowName = (i: number, n: number) =>
  n === 1 ? "Repositories" : i === 0 ? "Entry points" : i === n - 1 ? "Shared" : n === 3 ? "Services" : `Level ${i + 1}`;

/** Rows from entry points at the top to shared foundations at the bottom; each row wraps, so deep
 * and wide organisations both fit. Lines show what the repository under the pointer touches. */
export function LayersView({ map, onPick }: { map: OrgMap; onPick: (repo: string) => void }) {
  const { repos, layers } = useRepos(map);
  const box = useRef<HTMLDivElement>(null);
  const cards = useRef(new Map<string, HTMLButtonElement>());
  const [hover, setHover] = useState<string | null>(null);
  const [lines, setLines] = useState<{ key: string; d: string; contract: boolean; a: string; b: string }[]>([]);

  useLayoutEffect(() => {
    const measure = () => {
      const root = box.current?.getBoundingClientRect();
      if (!root) return;
      const out: typeof lines = [];
      for (const r of repos.values()) {
        for (const [to, es] of r.uses) {
          const a = cards.current.get(r.id)?.getBoundingClientRect();
          const b = cards.current.get(to)?.getBoundingClientRect();
          if (!a || !b) continue;
          const x1 = a.left + a.width / 2 - root.left;
          const x2 = b.left + b.width / 2 - root.left;
          let d: string;
          if (b.top > a.bottom) {
            const y1 = a.bottom - root.top;
            const y2 = b.top - root.top;
            const bend = Math.max(24, (y2 - y1) / 2);
            d = `M${x1},${y1} C${x1},${y1 + bend} ${x2},${y2 - bend} ${x2},${y2}`;
          } else {
            // Same row or upwards (a cycle): around the right-hand sides.
            const ax = a.right - root.left;
            const bx = b.right - root.left;
            const y1 = a.top + a.height / 2 - root.top;
            const y2 = b.top + b.height / 2 - root.top;
            const out2 = Math.max(ax, bx) + 28;
            d = `M${ax},${y1} C${out2},${y1} ${out2},${y2} ${bx},${y2}`;
          }
          out.push({ key: `${r.id}>${to}`, d, contract: contract(es), a: r.id, b: to });
        }
      }
      setLines(out);
    };
    measure();
    const ro = new ResizeObserver(measure);
    if (box.current) ro.observe(box.current);
    return () => ro.disconnect();
  }, [repos, layers]);

  const lit = (l: { a: string; b: string }) => hover !== null && (l.a === hover || l.b === hover);
  const near = (id: string) => hover === null || id === hover || repos.get(hover)?.uses.has(id) || repos.get(hover)?.usedBy.has(id);

  return (
    <div ref={box} className="relative px-5 py-5">
      <svg className="pointer-events-none absolute inset-0 h-full w-full overflow-visible" aria-hidden="true">
        <defs>
          <marker id="tip-code" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="7" markerHeight="7" orient="auto">
            <path d="M0,0 L8,4 L0,8 z" fill="var(--color-muted-foreground)" />
          </marker>
          <marker id="tip-contract" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="7" markerHeight="7" orient="auto">
            <path d="M0,0 L8,4 L0,8 z" fill="var(--color-brand)" />
          </marker>
        </defs>
        {[...lines]
          .sort((x, y) => Number(lit(x)) - Number(lit(y)))
          .map((l) => (
            <path
              key={l.key}
              d={l.d}
              fill="none"
              stroke={l.contract ? "var(--color-brand)" : "var(--color-muted-foreground)"}
              strokeWidth={lit(l) ? 2 : 1.25}
              strokeOpacity={hover === null ? 0.18 : lit(l) ? 0.95 : 0.04}
              markerEnd={lit(l) ? `url(#tip-${l.contract ? "contract" : "code"})` : undefined}
            />
          ))}
      </svg>
      <div className="relative flex flex-col gap-9">
        {layers.map((row, i) => (
          <div key={i} className="flex flex-col gap-2 md:flex-row md:items-center md:gap-4">
            <div className="eyebrow md:w-24 md:shrink-0">{rowName(i, layers.length)}</div>
            <div className="flex flex-1 flex-wrap justify-center gap-2.5">
              {row.map((r) => (
                <button
                  key={r.id}
                  ref={(el) => {
                    if (el) cards.current.set(r.id, el);
                    else cards.current.delete(r.id);
                  }}
                  onMouseEnter={() => setHover(r.id)}
                  onMouseLeave={() => setHover(null)}
                  onFocus={() => setHover(r.id)}
                  onBlur={() => setHover(null)}
                  onClick={() => onPick(r.id)}
                  title={`${r.id}: uses ${r.uses.size}, used by ${r.usedBy.size}`}
                  className={cn(
                    "flex min-h-10 items-center gap-2 rounded-lg border border-input bg-popover px-3 py-1.5 text-left transition-[opacity,border-color]",
                    hover === r.id ? "border-brand" : "hover:border-muted-foreground/50",
                    !near(r.id) && "opacity-30",
                  )}
                >
                  <RepoDot repo={r.id} />
                  <span className="text-[13px] font-medium whitespace-nowrap">{r.id}</span>
                  <span className="nums font-mono text-[11px] text-muted-foreground">{r.symbols}</span>
                </button>
              ))}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

/** Every pair at once: a row uses a column; contracts in green, code links in grey. */
export function MatrixView({ map, onPick }: { map: OrgMap; onPick: (repo: string) => void }) {
  const { layers } = useRepos(map);
  const order = layers.flat();
  const [at, setAt] = useState<[string, string] | null>(null);
  return (
    <div className="overflow-auto px-5 py-5">
      <table className="border-separate border-spacing-0 text-xs">
        <thead>
          <tr>
            <th className="sticky left-0 z-10 bg-card" />
            {order.map((c) => (
              <th key={c.id} className={cn("h-32 w-8 px-0 pb-2 align-bottom font-normal", at?.[1] === c.id && "text-foreground")}>
                <button
                  onClick={() => onPick(c.id)}
                  className="mx-auto block max-h-30 truncate font-mono text-[11px] text-muted-foreground [writing-mode:vertical-rl] rotate-180 hover:text-foreground"
                >
                  {c.id}
                </button>
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {order.map((r) => (
            <tr key={r.id}>
              <th className="sticky left-0 z-10 bg-card pr-3 text-right font-normal">
                <button
                  onClick={() => onPick(r.id)}
                  className={cn(
                    "flex w-full items-center justify-end gap-2 font-mono text-[11px] text-muted-foreground hover:text-foreground",
                    at?.[0] === r.id && "text-foreground",
                  )}
                >
                  {r.id} <RepoDot repo={r.id} />
                </button>
              </th>
              {order.map((c) => {
                const es = r.uses.get(c.id) ?? [];
                const n = count(es);
                const via = es.flatMap((e) => e.via ?? []);
                return (
                  <td
                    key={c.id}
                    onMouseEnter={() => setAt([r.id, c.id])}
                    onMouseLeave={() => setAt(null)}
                    title={
                      n
                        ? `${r.id} uses ${c.id}: ${n} link${n > 1 ? "s" : ""}${via.length ? ` (${via.slice(0, 6).join(", ")})` : ""}`
                        : undefined
                    }
                    className={cn(
                      "size-8 border-b border-r border-border/60 text-center",
                      r.id === c.id && "bg-muted/60",
                      (at?.[0] === r.id || at?.[1] === c.id) && r.id !== c.id && "bg-muted/40",
                    )}
                  >
                    {n > 0 && (
                      <span
                        className={cn(
                          "nums inline-flex min-w-6 justify-center rounded-[4px] px-1 py-0.5 font-mono text-[10.5px]",
                          contract(es) ? "bg-accent text-accent-foreground" : "bg-muted text-foreground",
                        )}
                      >
                        {n}
                      </span>
                    )}
                  </td>
                );
              })}
            </tr>
          ))}
        </tbody>
      </table>
      <p className="mt-3 text-xs text-muted-foreground">
        Read across: the row's repository uses the column's. Green cells go through service contracts.
      </p>
    </div>
  );
}

/** One repository: what uses it on the left, what it uses on the right, with the contracts in between. */
export function FocusView({
  map,
  repo,
  reach,
  onPick,
  onBack,
}: {
  map: OrgMap;
  repo: string;
  reach: Map<string, number>;
  onPick: (repo: string) => void;
  onBack: () => void;
}) {
  const { repos } = useRepos(map);
  const r = repos.get(repo);
  if (!r) return null;
  const side = (title: string, links: Map<string, Edge[]>, empty: string) => (
    <div className="flex min-w-0 flex-1 flex-col gap-2">
      <div className="eyebrow">{title}</div>
      {links.size === 0 && <p className="text-xs text-muted-foreground">{empty}</p>}
      {[...links].map(([other, es]) => {
        const via = [...new Set(es.flatMap((e) => e.via ?? []))];
        return (
          <button
            key={other}
            onClick={() => onPick(other)}
            className="flex flex-col gap-1.5 rounded-lg border border-input bg-popover px-3 py-2 text-left transition-colors hover:border-muted-foreground/50"
          >
            <span className="flex items-center gap-2">
              <RepoDot repo={other} />
              <span className="min-w-0 flex-1 truncate text-[13px] font-medium">{other}</span>
              <span className="nums font-mono text-[11px] text-muted-foreground">{count(es)}</span>
            </span>
            {via.length > 0 ? (
              <span className="flex flex-wrap gap-1">
                {via.slice(0, 4).map((v) => (
                  <span key={v} className="rounded bg-accent px-1.5 py-0.5 font-mono text-[10.5px] text-accent-foreground">
                    {v}
                  </span>
                ))}
                {via.length > 4 && <span className="font-mono text-[10.5px] text-muted-foreground">+{via.length - 4}</span>}
              </span>
            ) : (
              <span className="font-mono text-[10.5px] text-muted-foreground">code dependency</span>
            )}
          </button>
        );
      })}
    </div>
  );
  return (
    <div className="flex flex-col gap-4 px-5 py-5">
      <button onClick={onBack} className="flex w-fit items-center gap-1.5 text-xs text-muted-foreground hover:text-foreground">
        <ArrowLeft className="size-3.5" /> All repositories
      </button>
      <div className="flex flex-col gap-4 lg:flex-row lg:items-start">
        {side("Used by", r.usedBy, "No other indexed repository uses it.")}
        <div className="flex flex-col items-center gap-2 lg:w-56 lg:pt-6">
          <div className="flex w-full flex-col gap-1 rounded-lg border border-brand bg-popover px-4 py-3 shadow-[0_0_0_4px_color-mix(in_srgb,var(--color-brand)_14%,transparent)]">
            <span className="flex items-center gap-2 text-sm font-semibold">
              <RepoDot repo={r.id} /> {r.id}
            </span>
            <span className="text-xs text-muted-foreground">{r.symbols.toLocaleString()} symbols</span>
          </div>
          <p className="text-center text-xs text-muted-foreground">
            {reach.size === 0
              ? "A change here reaches no other repository."
              : `A change here can reach ${reach.size} ${reach.size === 1 ? "repository" : "repositories"}${
                  Math.max(...reach.values()) > 1 ? `, up to ${Math.max(...reach.values())} hops away` : ""
                }.`}
          </p>
        </div>
        {side("Uses", r.uses, "It uses no other indexed repository.")}
      </div>
    </div>
  );
}
