import { useEffect, useMemo, useState } from "react";
import { ConfigProvider, theme as antdTheme } from "antd";
import { Routes, Route, Navigate, useLocation } from "react-router-dom";
import { AppShell } from "./layout/AppShell";
import { DashboardPage } from "./pages/DashboardPage";
import { NotificationsPage } from "./pages/NotificationsPage";
import { ProfilesPage } from "./pages/ProfilesPage";
import { AccountsPage } from "./pages/AccountsPage";
import { FlowsListPage } from "./pages/FlowsListPage";
import { FlowEditorPage } from "./pages/FlowEditorPage";
import { FlowDetailPage } from "./pages/FlowDetailPage";
import { FlowActionsCatalogPage } from "./pages/FlowActionsCatalogPage";
import { AtomicActionBuilderPage } from "./pages/AtomicActionBuilderPage";
import { SchedulesPage } from "./pages/SchedulesPage";
import { HistoryPage } from "./pages/HistoryPage";

export default function App() {
  const [colorMode, setColorMode] = useState<"light" | "dark">(() => {
    const saved = localStorage.getItem("ui.colorMode");
    return saved === "dark" ? "dark" : "light";
  });
  const title = useTitleFromRoute();

  useEffect(() => {
    // reserved for global hotkeys if needed
  }, []);

  useEffect(() => {
    localStorage.setItem("ui.colorMode", colorMode);
    document.documentElement.dataset.theme = colorMode;
  }, [colorMode]);

  return (
    <ConfigProvider
      theme={{
        algorithm:
          colorMode === "dark"
            ? antdTheme.darkAlgorithm
            : antdTheme.defaultAlgorithm,
        token: {
          colorPrimary: "#1677ff",
          borderRadius: 8,
          fontSize: 13,
        },
      }}
    >
      <AppShell
        title={title}
        colorMode={colorMode}
        onToggleColorMode={() =>
          setColorMode((m) => (m === "dark" ? "light" : "dark"))
        }
      >
        <Routes>
          <Route path="/" element={<Navigate to="/dashboard" replace />} />
          <Route path="/dashboard" element={<DashboardPage />} />
          <Route path="/notifications" element={<NotificationsPage />} />
          <Route path="/profiles" element={<ProfilesPage />} />
          <Route path="/accounts" element={<AccountsPage />} />
          <Route path="/flows/actions/build" element={<AtomicActionBuilderPage />} />
          <Route path="/flows/actions" element={<FlowActionsCatalogPage />} />
          <Route path="/flows/new" element={<FlowEditorPage />} />
          <Route path="/flows/:id/edit" element={<FlowEditorPage />} />
          <Route path="/flows/:id/view" element={<FlowDetailPage />} />
          <Route path="/flows" element={<FlowsListPage />} />
          <Route path="/schedules" element={<SchedulesPage />} />
          <Route path="/history" element={<HistoryPage />} />
          <Route path="*" element={<Navigate to="/dashboard" replace />} />
        </Routes>
      </AppShell>
    </ConfigProvider>
  );
}

function useTitleFromRoute(): string {
  const loc = useLocation();
  return useMemo(() => {
    const p = loc.pathname.toLowerCase();
    if (p.startsWith("/dashboard")) return "Dashboard";
    if (p.startsWith("/notifications")) return "Notifications";
    if (p.startsWith("/profiles")) return "Profile";
    if (p.startsWith("/accounts")) return "Account";
    if (p.startsWith("/flows/actions/build")) return "Tạo / sửa action (atomic)";
    if (p.startsWith("/flows/actions")) return "Flow actions";
    if (p.startsWith("/flows")) return "Flows";
    if (p.startsWith("/schedules")) return "Schedule";
    if (p.startsWith("/history")) return "Run History";
    return "Dashboard";
  }, [loc.pathname]);
}
