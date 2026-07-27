import {
  AppstoreOutlined,
  BranchesOutlined,
  CloudUploadOutlined,
  FolderOutlined,
  MoonFilled,
  NodeIndexOutlined,
  OrderedListOutlined,
  PictureOutlined,
  SettingOutlined,
  SunOutlined,
  UserOutlined,
} from "@ant-design/icons";
import { useQuery } from "@tanstack/react-query";
import { Badge, Layout, Menu, Space, Switch, Typography } from "antd";
import { useState } from "react";
import { Outlet, useLocation, useNavigate } from "react-router-dom";
import { AgentLogDrawer } from "@/features/agents/AgentLogDrawer";
import { useThemeMode } from "@/theme/ThemeProvider";
import { api } from "@/lib/api/client";

const NAV_ITEMS = [
  { key: "/", label: "Dashboard", icon: <AppstoreOutlined /> },
  { key: "/dag-pipeline", label: "Smart Pipeline", icon: <BranchesOutlined /> },
  { key: "/pipeline", label: "Studio", icon: <NodeIndexOutlined /> },
  { key: "/projects", label: "Projects", icon: <FolderOutlined /> },
  { key: "/characters", label: "Nhân vật", icon: <UserOutlined /> },
  { key: "/scenes", label: "Scenes", icon: <PictureOutlined /> },
  { key: "/media", label: "Media", icon: <CloudUploadOutlined /> },
  { key: "/settings", label: "Cài đặt", icon: <SettingOutlined /> },
];

export function AppShell() {
  const location = useLocation();
  const navigate = useNavigate();
  const { themeMode, toggleTheme } = useThemeMode();
  const [logOpen, setLogOpen] = useState(false);

  // Poll agent log count for badge (only when drawer is closed)
  const logQ = useQuery({
    queryKey: ["agent-log"],
    queryFn: () => api.listAgentLog(),
    refetchInterval: 5_000,
    select: (data) => data.filter((e) => e.status === "active").length,
  });
  const activeCount = logQ.data ?? 0;

  const path = location.pathname;
  const selectedKey =
    NAV_ITEMS.slice()
      .reverse()
      .find((item) => item.key !== "/" && path.startsWith(item.key))?.key ??
    (path === "/" ? "/" : "/pipeline");

  return (
    <Layout className="app-shell">
      <Layout.Sider width={220} className="app-nav" theme="light">
        <div className="app-brand">
          <Typography.Title level={5} style={{ margin: 0 }}>
            Flow Agent
          </Typography.Title>
          <Typography.Text className="app-brand-sub">Video · Multi-Agent</Typography.Text>
        </div>

        <Menu
          mode="inline"
          selectedKeys={[selectedKey]}
          items={NAV_ITEMS}
          onClick={(e) => navigate(e.key)}
          style={{ border: "none", background: "transparent", flex: 1 }}
        />

        <div style={{ marginTop: "auto", paddingTop: 16 }}>
          {/* Agent Log trigger */}
          <div
            onClick={() => setLogOpen((o) => !o)}
            style={{
              display: "flex",
              alignItems: "center",
              gap: 8,
              padding: "8px 16px",
              cursor: "pointer",
              borderRadius: 6,
              marginBottom: 8,
              background: logOpen ? "var(--bg)" : "transparent",
              border: logOpen ? "1px solid var(--border)" : "1px solid transparent",
              color: logOpen ? "var(--accent)" : "var(--muted)",
              fontSize: 13,
              transition: "all 0.15s",
            }}
          >
            <Badge count={activeCount} size="small" offset={[4, 0]}>
              <OrderedListOutlined style={{ fontSize: 15 }} />
            </Badge>
            <span>Agent Log</span>
          </div>

          <Space align="center" size={8} style={{ paddingLeft: 16 }}>
            {themeMode === "dark" ? (
              <MoonFilled style={{ color: "var(--accent)" }} />
            ) : (
              <SunOutlined style={{ color: "var(--warn)" }} />
            )}
            <Switch
              size="small"
              checked={themeMode === "dark"}
              checkedChildren="Dark"
              unCheckedChildren="Light"
              onChange={toggleTheme}
            />
          </Space>
        </div>
      </Layout.Sider>

      <Layout.Content
        className="app-main"
        style={{
          padding: "8px 16px",
          marginRight: logOpen ? 300 : 0,
          transition: "margin-right 0.2s",
        }}
      >
        <Outlet />
      </Layout.Content>

      <AgentLogDrawer open={logOpen} onClose={() => setLogOpen(false)} />
    </Layout>
  );
}
