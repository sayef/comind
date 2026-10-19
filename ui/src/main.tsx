import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { BrowserRouter, Route, Routes } from "react-router";
import { Layout } from "@/components/layout";
import { ThemeProvider } from "@/components/theme";
import { TooltipProvider } from "@/components/ui/tooltip";
import { ContractsPage } from "@/pages/contracts";
import { ConventionsPage } from "@/pages/conventions";
import { ExplorerPage } from "@/pages/explorer";
import { LedgerPage } from "@/pages/ledger";
import { OverviewPage } from "@/pages/overview";
import { PackagesPage } from "@/pages/packages";
import { RepositoriesPage } from "@/pages/repositories";
import { SettingsPage } from "@/pages/settings";
import { SetupPage } from "@/pages/setup";
import "./index.css";

const queries = new QueryClient({ defaultOptions: { queries: { staleTime: 60_000, retry: 1 } } });

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <ThemeProvider>
      <QueryClientProvider client={queries}>
        <TooltipProvider>
          <BrowserRouter>
            <Routes>
              <Route element={<Layout />}>
                <Route index element={<OverviewPage />} />
                <Route path="contracts" element={<ContractsPage />} />
                <Route path="ledger" element={<LedgerPage />} />
                <Route path="explorer" element={<ExplorerPage />} />
                <Route path="conventions" element={<ConventionsPage />} />
                <Route path="packages" element={<PackagesPage />} />
                <Route path="setup" element={<SetupPage />} />
                <Route path="settings" element={<SettingsPage />} />
                <Route path="repositories" element={<RepositoriesPage />} />
                <Route path="*" element={<OverviewPage />} />
              </Route>
            </Routes>
          </BrowserRouter>
        </TooltipProvider>
      </QueryClientProvider>
    </ThemeProvider>
  </StrictMode>,
);
