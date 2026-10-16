import Graph from "graphology";
import forceAtlas2 from "graphology-layout-forceatlas2";
import { useEffect, useRef } from "react";
import Sigma from "sigma";
import { drawDiscNodeLabel, type NodeHoverDrawingFunction } from "sigma/rendering";
import { useTheme } from "@/components/theme";
import { CONTRACTS, repoHue } from "@/lib/utils";

export interface GraphNode {
  id: string;
  label: string;
  repo: string;
  size?: number;
}

export interface GraphEdge {
  src: string;
  dst: string;
  kind: string;
  weight?: number;
}

interface Props {
  nodes: GraphNode[];
  edges: GraphEdge[];
  focus?: string;
  /** Highlighted node ids (everything else is dimmed); `null` shows all. */
  highlight?: Set<string> | null;
  onPick?: (id: string) => void;
  className?: string;
}

function hslToHex(h: number, s: number, l: number) {
  const a = (s / 100) * Math.min(l / 100, 1 - l / 100);
  const f = (n: number) => {
    const k = (n + h / 30) % 12;
    const c = l / 100 - a * Math.max(Math.min(k - 3, 9 - k, 1), -1);
    return Math.round(255 * c)
      .toString(16)
      .padStart(2, "0");
  };
  return `#${f(0)}${f(8)}${f(4)}`;
}

/** Sigma's hover box is hard-coded white; this one follows the theme. */
const hoverDrawer =
  (dark: boolean): NodeHoverDrawingFunction =>
  (ctx, data, settings) => {
    ctx.font = `${settings.labelWeight} ${settings.labelSize}px ${settings.labelFont}`;
    const pad = 3;
    const h = settings.labelSize + 2 * pad;
    const r = Math.max(data.size, h / 2) + pad;
    const w = data.label ? ctx.measureText(data.label).width + r + 2 * pad : 0;
    ctx.fillStyle = dark ? "#1f2633" : "#ffffff";
    ctx.shadowColor = dark ? "rgba(0,0,0,0.6)" : "rgba(15,23,42,0.25)";
    ctx.shadowBlur = 8;
    ctx.beginPath();
    ctx.arc(data.x, data.y, r, 0, Math.PI * 2);
    if (w) ctx.roundRect(data.x, data.y - h / 2, w, h, 4);
    ctx.fill();
    ctx.shadowBlur = 0;
    drawDiscNodeLabel(ctx, data, settings);
  };

const nodeColor = (repo: string) => (repo === CONTRACTS ? "#d9a52b" : hslToHex(repoHue(repo), 55, 52));

/** A WebGL graph: ForceAtlas2 layout, nodes coloured by repository, contracts in amber. */
export function GraphView({ nodes, edges, focus, highlight, onPick, className }: Props) {
  const container = useRef<HTMLDivElement>(null);
  const sigma = useRef<Sigma | null>(null);
  const hovered = useRef<string | null>(null);
  const lit = useRef<Set<string> | null>(null);
  const { theme } = useTheme();
  const pick = useRef(onPick);
  pick.current = onPick;

  useEffect(() => {
    if (!container.current) return;
    const dark = theme === "dark";
    const g = new Graph({ type: "directed", multi: false });
    nodes.forEach((n, i) => {
      const angle = (i / Math.max(1, nodes.length)) * Math.PI * 2;
      g.addNode(n.id, {
        label: n.label,
        x: n.id === focus ? 0 : Math.cos(angle) * 10,
        y: n.id === focus ? 0 : Math.sin(angle) * 10,
        size: n.size ?? (n.id === focus ? 12 : 6),
        color: nodeColor(n.repo),
        repo: n.repo,
      });
    });
    const maxW = Math.max(1, ...edges.map((e) => e.weight ?? 1));
    for (const e of edges) {
      if (!g.hasNode(e.src) || !g.hasNode(e.dst) || g.hasEdge(e.src, e.dst)) continue;
      g.addEdge(e.src, e.dst, {
        kind: e.kind,
        size: e.weight ? 1 + (Math.log(e.weight + 1) / Math.log(maxW + 1)) * 4 : 1.2,
        color: e.kind === "contract" || e.kind === "counterpart" ? (dark ? "#b8892a" : "#e0b65a") : dark ? "#55627a" : "#b7c0cf",
        type: "arrow",
      });
    }
    if (g.order > 1) {
      forceAtlas2.assign(g, {
        iterations: 300,
        settings: { ...forceAtlas2.inferSettings(g), gravity: 1.2, scalingRatio: 12, barnesHutOptimize: g.order > 400 },
      });
    }

    const renderer = new Sigma(g, container.current, {
      renderEdgeLabels: false,
      defaultEdgeType: "arrow",
      labelFont: "Inter, ui-sans-serif, system-ui",
      labelSize: 12,
      labelWeight: "500",
      labelColor: { color: dark ? "#d5dbe5" : "#2b3445" },
      labelRenderedSizeThreshold: 5,
      zIndex: true,
      stagePadding: 48,
      defaultDrawNodeHover: hoverDrawer(dark),
      nodeReducer: (node, data) => {
        const on = lit.current;
        const hover = hovered.current;
        const res = { ...data };
        if (node === focus || node === hover) {
          res.highlighted = true;
          res.zIndex = 2;
        }
        if (on && !on.has(node) && node !== focus) {
          res.color = dark ? "#2f3747" : "#e3e7ee";
          res.label = "";
          res.zIndex = 0;
        }
        if (hover && node !== hover && !g.areNeighbors(node, hover)) {
          res.label = "";
        }
        return res;
      },
      edgeReducer: (edge, data) => {
        const on = lit.current;
        const [s, t] = g.extremities(edge);
        const res = { ...data };
        if (on && (!(on.has(s) || s === focus) || !(on.has(t) || t === focus))) {
          res.color = dark ? "#262d3a" : "#eef1f5";
        }
        const hover = hovered.current;
        if (hover && s !== hover && t !== hover) res.color = dark ? "#2a3140" : "#edf0f4";
        return res;
      },
    });
    renderer.on("enterNode", ({ node }) => {
      hovered.current = node;
      if (container.current) container.current.style.cursor = "pointer";
      renderer.refresh({ skipIndexation: true });
    });
    renderer.on("leaveNode", () => {
      hovered.current = null;
      if (container.current) container.current.style.cursor = "default";
      renderer.refresh({ skipIndexation: true });
    });
    renderer.on("clickNode", ({ node }) => pick.current?.(node));
    sigma.current = renderer;
    return () => {
      renderer.kill();
      sigma.current = null;
    };
  }, [nodes, edges, focus, theme]);

  useEffect(() => {
    lit.current = highlight ?? null;
    sigma.current?.refresh({ skipIndexation: true });
  }, [highlight]);

  return <div ref={container} className={className ?? "h-[420px] w-full"} />;
}
