import { useQuery } from "@tanstack/react-query";
import { BookMarked, Boxes, FolderGit2, LayoutDashboard, Moon, Network, Package, Plug, Ruler, Search, Settings, Sun } from "lucide-react";
import { useEffect, useState } from "react";
import { NavLink, Outlet } from "react-router";
import { AwsBanner } from "@/components/aws-banner";
import { CommandPalette } from "@/components/command-palette";
import { useTheme } from "@/components/theme";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { api } from "@/lib/api";
import { cn } from "@/lib/utils";

const NAV = [
  { to: "/", label: "Overview", icon: LayoutDashboard },
  { to: "/contracts", label: "Contracts", icon: Network },
  { to: "/ledger", label: "Ledger", icon: BookMarked },
  { to: "/conventions", label: "Conventions", icon: Ruler },
  { to: "/packages", label: "Packages", icon: Package },
  { to: "/repositories", label: "Repositories", icon: FolderGit2 },
  { to: "/setup", label: "Connect", icon: Plug },
  { to: "/settings", label: "Settings", icon: Settings },
];

export function Layout() {
  const { theme, toggle } = useTheme();
  const [palette, setPalette] = useState(false);
  const overview = useQuery({ queryKey: ["overview"], queryFn: api.overview });

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setPalette((p) => !p);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const inbox = overview.data
    ? Object.entries(overview.data.ledger)
        .filter(([k]) => k === "stale" || k === "orphaned")
        .reduce((n, [, v]) => n + v, 0)
    : 0;

  if (overview.error?.message.includes("access token")) {
    return (
      <div className="flex h-full items-center justify-center p-6">
        <div className="max-w-md text-center">
          <img src="/logo.svg" alt="" className="mx-auto size-10 dark:hidden" />
          <img src="/logo-dark.svg" alt="" className="mx-auto hidden size-10 dark:block" />
          <h1 className="mt-4 text-lg font-semibold">Open comindly from its link</h1>
          <p className="mt-2 text-sm text-muted-foreground">
            The UI only answers browsers that hold its access token. Run <code className="font-mono">comindly ui --web --open</code>, or open
            the link it prints once; after that, this address works as a bookmark.
          </p>
        </div>
      </div>
    );
  }

  return (
    <div className="flex h-full">
      <aside className="hidden w-60 shrink-0 flex-col border-r bg-sidebar md:flex">
        <div className="flex h-14 items-center gap-2.5 px-5">
          <img src="/logo.svg" alt="" className="size-7 dark:hidden" />
          <img src="/logo-dark.svg" alt="" className="hidden size-7 dark:block" />
          <span className="text-[15px] font-semibold tracking-tight">comindly</span>
        </div>
        <nav className="flex flex-1 flex-col gap-0.5 px-3 py-2">
          {NAV.map(({ to, label, icon: Icon }) => (
            <NavLink
              key={to}
              to={to}
              end={to === "/"}
              className={({ isActive }) =>
                cn(
                  "flex items-center gap-2.5 rounded-md px-3 py-2 text-sm text-muted-foreground transition-colors hover:bg-muted hover:text-foreground",
                  isActive && "bg-card font-medium text-foreground shadow-xs ring-1 ring-border",
                )
              }
            >
              <Icon className="size-4" />
              <span className="flex-1">{label}</span>
              {label === "Ledger" && inbox > 0 && <Badge variant="warning">{inbox}</Badge>}
            </NavLink>
          ))}
        </nav>
        <div className="border-t px-5 py-4 text-xs text-muted-foreground">
          {overview.data ? (
            <div className="flex items-center gap-2">
              <Boxes className="size-3.5" />
              {overview.data.repos.length} repos · {overview.data.symbols.toLocaleString()} symbols
            </div>
          ) : (
            "Loading index…"
          )}
        </div>
      </aside>

      <div className="flex min-w-0 flex-1 flex-col">
        <header className="flex h-14 shrink-0 items-center gap-3 border-b px-4 md:grid md:grid-cols-[1fr_minmax(0,32rem)_1fr] md:px-6">
          <div className="flex shrink-0 items-center">
            <span className="md:hidden">
              <img src="/logo.svg" alt="" className="size-6 dark:hidden" />
              <img src="/logo-dark.svg" alt="" className="hidden size-6 dark:block" />
            </span>
          </div>
          <button
            onClick={() => setPalette(true)}
            className="flex h-9 w-full min-w-0 items-center gap-2 rounded-md border bg-card px-3 text-sm text-muted-foreground shadow-xs transition-colors hover:bg-muted"
          >
            <Search className="size-4 shrink-0" />
            <span className="flex-1 truncate text-left">Search symbols, files, contracts…</span>
            <kbd className="hidden rounded border bg-muted px-1.5 font-mono text-[10px] sm:inline">⌘K</kbd>
          </button>
          <div className="flex shrink-0 items-center justify-end gap-2">
            {overview.data && !overview.data.writable && <Badge variant="outline">read-only</Badge>}
            <Button variant="ghost" size="icon" onClick={toggle} aria-label="Toggle theme">
              {theme === "dark" ? <Sun /> : <Moon />}
            </Button>
          </div>
        </header>
        <nav className="flex gap-1 overflow-x-auto border-b px-4 py-2 md:hidden">
          {NAV.map(({ to, label }) => (
            <NavLink
              key={to}
              to={to}
              end={to === "/"}
              className={({ isActive }) =>
                cn("rounded-md px-3 py-1.5 text-xs whitespace-nowrap", isActive ? "bg-muted font-medium" : "text-muted-foreground")
              }
            >
              {label}
            </NavLink>
          ))}
        </nav>
        <AwsBanner remote={!!overview.data?.root?.startsWith("s3://")} />
        <main className="min-h-0 flex-1 overflow-y-auto">
          <div className="mx-auto w-full min-w-0 max-w-7xl px-4 py-6 md:px-8">
            <Outlet />
          </div>
        </main>
      </div>
      <CommandPalette open={palette} onOpenChange={setPalette} />
    </div>
  );
}
