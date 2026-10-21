import { MutationCache, QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { BrowserRouter, Navigate, Route, Routes } from "react-router";
import { Layout } from "@/components/layout";
import { Toasts, announce } from "@/components/toasts";
import { ThemeProvider } from "@/components/theme";
import { TooltipProvider } from "@/components/ui/tooltip";
import { ContractsPage } from "@/pages/contracts";
import { ConventionsPage } from "@/pages/conventions";
import { ExplorerPage } from "@/pages/explorer";
import { FlowsPage } from "@/pages/flows";
import { LedgerPage } from "@/pages/ledger";
import { OverviewPage } from "@/pages/overview";
import { PackagesPage } from "@/pages/packages";
import { RepositoriesPage } from "@/pages/repositories";
import { SettingsPage, SettingsShell } from "@/pages/settings";
import { SetupPage } from "@/pages/setup";
import "./index.css";

const queries = new QueryClient({
  defaultOptions: { queries: { staleTime: 60_000, retry: 1 } },
  mutationCache: new MutationCache({ onSuccess: (data, _v, _c, m) => announce(data, null, m.meta), onError: (e, _v, _c, m) => announce(null, e, m.meta) }),
});

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <ThemeProvider>
      <QueryClientProvider client={queries}>
        <TooltipProvider>
          <Toasts />
          <BrowserRouter>
            <Routes>
              <Route element={<Layout />}>
                <Route index element={<OverviewPage />} />
                <Route path="contracts" element={<ContractsPage />} />
                <Route path="ledger" element={<LedgerPage />} />
                <Route path="explorer" element={<ExplorerPage />} />
                <Route path="conventions" element={<ConventionsPage />} />
                <Route path="packages" element={<PackagesPage />} />
                <Route path="flows" element={<FlowsPage />} />
                <Route path="setup" element={<SetupPage />} />
                <Route path="settings" element={<SettingsShell />}>
                  <Route index element={<SettingsPage />} />
                  <Route path="repositories" element={<RepositoriesPage />} />
                </Route>
                <Route path="repositories" element={<Navigate to="/settings/repositories" replace />} />
                <Route path="*" element={<OverviewPage />} />
              </Route>
            </Routes>
          </BrowserRouter>
        </TooltipProvider>
      </QueryClientProvider>
    </ThemeProvider>
  </StrictMode>,
);
