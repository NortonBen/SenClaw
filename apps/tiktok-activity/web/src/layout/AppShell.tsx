import type { ReactNode } from "react";
import { Badge, Button, Drawer, Layout, List, Menu, Space, Typography } from "antd";
import {
  BellOutlined,
  ClusterOutlined,
  DashboardOutlined,
  DatabaseOutlined,
  HistoryOutlined,
  MoonOutlined,
  SunOutlined,
  SettingOutlined,
  UserOutlined,
} from "@ant-design/icons";
import { useEffect, useState } from "react";
import { api } from "../api";
import type { Notification } from "../types/notifications";
import { useLocation, useNavigate } from "react-router-dom";

export type NavKey =
  | "dashboard"
  | "settings"
  | "notifications"
  | "proxies"
  | "profiles"
  | "accounts"
  | "flows"
  | "schedules"
  | "history"
  | "skills";

type Props = {
  title?: string;
  colorMode: "light" | "dark";
  onToggleColorMode: () => void;
  children: ReactNode;
};

export function AppShell({
  title,
  colorMode,
  onToggleColorMode,
  children,
}: Props) {
  const nav = useNavigate();
  const loc = useLocation();
  const active = routeToNavKey(loc.pathname);

  const [notifOpen, setNotifOpen] = useState(false);
  const [notifCount, setNotifCount] = useState(0);
  const [notifs, setNotifs] = useState<Notification[]>([]);

  useEffect(() => {
    const load = async () => {
      try {
        const r = await api<{ count: number }>("/api/notifications/unread-count");
        setNotifCount(r.count);
      } catch {
        // ignore
      }
    };
    void load();
    const t = window.setInterval(load, 5000);
    return () => window.clearInterval(t);
  }, []);

  const openNotifications = async () => {
    setNotifOpen(true);
    const list = await api<Notification[] | null>("/api/notifications?unread=1");
    setNotifs(normalizeNotifications(list));
  };

  const markRead = async (id: string) => {
    await fetch(`/api/notifications/${encodeURIComponent(id)}/read`, { method: "POST" });
    const list = await api<Notification[] | null>("/api/notifications?unread=1");
    setNotifs(normalizeNotifications(list));
    const r = await api<{ count: number }>("/api/notifications/unread-count");
    setNotifCount(r.count);
  };

  const markAll = async () => {
    await fetch(`/api/notifications/read-all`, { method: "POST" });
    setNotifs([]);
    setNotifCount(0);
  };

  return (
    <Layout style={{ minHeight: "100vh", background: "var(--app-content-bg)" }}>
      <Layout.Sider theme="dark" width={260} style={{ background: "#0b2239" }}>
        <div style={{ padding: 14 }}>
          <div className="sider-brand" style={{ display: "flex", alignItems: "center", gap: 12 }}>
            <img src="/logo.svg" alt="" width={40} height={40} style={{ flexShrink: 0 }} />
            <div>
              <Typography.Text strong style={{ color: "#e2e8f0", display: "block", lineHeight: 1.25, fontSize: 18 }}>
                TikTok Activity
              </Typography.Text>
            </div>
          </div>
        </div>
        <Menu
          theme="dark"
          mode="inline"
          selectedKeys={[active]}
          onClick={(e) => nav(navKeyToRoute(e.key as NavKey))}
          className="side-menu"
          items={[
            { key: "dashboard", icon: <DashboardOutlined />, label: "DASHBOARD" },
            {
              type: "group",
              key: "grp-device",
              label: "ACCOUNT MANAGEMENT",
              children: [
                { key: "accounts", icon: <UserOutlined />, label: "ACCOUNTS" },
                { key: "profiles", icon: <DatabaseOutlined />, label: "PROFILES" },
              ],
            },
            {
              type: "group",
              key: "grp-auto",
              label: "AUTOMATION",
              children: [
                { key: "flows", icon: <ClusterOutlined />, label: "FLOWS" },
                { key: "schedules", icon: <SettingOutlined />, label: "SCHEDULES" },
                { key: "notifications", icon: <BellOutlined />, label: "NOTIFICATIONS" },
                { key: "history", icon: <HistoryOutlined />, label: "RUN HISTORY" },
              ],
            },
          ]}
        />
      </Layout.Sider>

      <Layout style={{ background: "var(--app-content-bg)" }}>
        <Layout.Header className="topbar">
          <div className="topbar-inner">
            <Typography.Text className="topbar-title">
              {title ?? "Dashboard"}
            </Typography.Text>
            <Space size={10}>
              <Badge count={notifCount} size="small">
                <Button
                  type="text"
                  className="topbar-btn"
                  icon={<BellOutlined />}
                  onClick={() => void openNotifications()}
                />
              </Badge>
              <Button
                type="text"
                className="topbar-btn"
                icon={colorMode === "dark" ? <SunOutlined /> : <MoonOutlined />}
                onClick={onToggleColorMode}
              >
                {colorMode === "dark" ? "Light" : "Dark"}
              </Button>
            </Space>
          </div>
        </Layout.Header>
        <Layout.Content style={{ padding: 16, background: "var(--app-content-bg)", minHeight: "calc(100vh - 64px)" }}>
          {children}
        </Layout.Content>
      </Layout>
      <Drawer
        title="Notifications"
        open={notifOpen}
        onClose={() => setNotifOpen(false)}
        width={420}
        extra={
          <Button onClick={() => void markAll()} disabled={notifCount === 0}>
            Mark all read
          </Button>
        }
      >
        <List
          dataSource={notifs}
          locale={{ emptyText: "Không có notification chưa đọc." }}
          renderItem={(n) => (
            <List.Item
              actions={[
                <Button key="read" type="link" onClick={() => void markRead(n.id)}>
                  Mark read
                </Button>,
              ]}
            >
              <List.Item.Meta title={n.title} description={n.body} />
            </List.Item>
          )}
        />
      </Drawer>
    </Layout>
  );
}

function routeToNavKey(pathname: string): NavKey {
  const p = pathname.toLowerCase();
  if (p === "/" || p.startsWith("/dashboard")) return "dashboard";
  if (p.startsWith("/settings")) return "settings";
  if (p.startsWith("/agent-skills")) return "skills";
  if (p.startsWith("/notifications")) return "notifications";
  if (p.startsWith("/proxies")) return "proxies";
  if (p.startsWith("/profiles")) return "profiles";
  if (p.startsWith("/accounts")) return "accounts";
  if (p.startsWith("/flows")) return "flows";
  if (p.startsWith("/schedules")) return "schedules";
  if (p.startsWith("/history")) return "history";
  return "dashboard";
}

function navKeyToRoute(k: NavKey): string {
  switch (k) {
    case "dashboard":
      return "/dashboard";
    case "settings":
      return "/settings";
    case "skills":
      return "/agent-skills";
    case "notifications":
      return "/notifications";
    case "proxies":
      return "/proxies";
    case "profiles":
      return "/profiles";
    case "accounts":
      return "/accounts";
    case "flows":
      return "/flows";
    case "schedules":
      return "/schedules";
    case "history":
      return "/history";
  }
}

function normalizeNotifications(value: Notification[] | null): Notification[] {
  return Array.isArray(value) ? value : [];
}

