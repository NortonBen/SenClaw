import { Badge, Layout, Menu, Space, Typography } from "antd";
import {
  BookOutlined,
  SettingOutlined,
  ThunderboltOutlined,
} from "@ant-design/icons";
import { Navigate, Route, Routes, useLocation, useNavigate } from "react-router-dom";
import { useQuery, useQueryClient } from "@tanstack/react-query";

import Library from "./pages/Library";
import StoryDetail from "./pages/StoryDetail";
import Processes from "./pages/Processes";
import Settings from "./pages/Settings";
import { api } from "./lib/api";
import { useDashboardWS } from "./lib/ws";

const { Header, Content, Footer } = Layout;

export default function App() {
  const nav = useNavigate();
  const { pathname } = useLocation();
  const qc = useQueryClient();

  // Lightweight live counter for the header badge.
  const health = useQuery({
    queryKey: ["health"],
    queryFn: api.health,
    refetchInterval: 8000,
  });
  const active =
    (Number(health.data?.queued) || 0) + (Number(health.data?.processing) || 0);

  // Worker events drive the UI so it never has to poll.
  //
  // `process:delta` fires once per finished chunk and carries the updated row's
  // identity but not the row — invalidating the whole list there would refetch
  // every process (up to 200 rows, prompts included) hundreds of times over a
  // long novel. Only status transitions justify a refetch.
  useDashboardWS((e) => {
    if (!e.type.startsWith("process:")) return;

    if (e.type !== "process:delta") {
      qc.invalidateQueries({ queryKey: ["processes"] });
      qc.invalidateQueries({ queryKey: ["health"] });
    }

    // The split is persisted on the first `processing` tick, flipping the
    // detail page's "this is only a preview" banner from true to false. Without
    // this the banner keeps telling the user to go change chunk settings that
    // are already frozen — for the whole run.
    const storyId = (e.data as { story_id?: number }).story_id;
    if (typeof storyId === "number") {
      qc.invalidateQueries({ queryKey: ["stories", storyId, "chunks"] });
    }

    if (e.type === "process:complete") {
      qc.invalidateQueries({ queryKey: ["stories"] });
    }
  });

  const selected = pathname.startsWith("/processes")
    ? "/processes"
    : pathname.startsWith("/settings")
      ? "/settings"
      : "/stories";

  return (
    <Layout className="rs-layout">
      <Header className="rs-header">
        <div className="rs-brand">
          <span className="rs-brand-badge">✍️</span>
          <span>Rewrite Story</span>
        </div>
        <Menu
          theme="dark"
          mode="horizontal"
          selectedKeys={[selected]}
          onClick={({ key }) => nav(key)}
          style={{ flex: 1, minWidth: 0, background: "transparent", borderBottom: "none" }}
          items={[
            { key: "/stories", icon: <BookOutlined />, label: "Kho truyện" },
            {
              key: "/processes",
              icon: <ThunderboltOutlined />,
              label: (
                <Space size={8}>
                  Tiến trình
                  {active > 0 && <Badge count={active} color="#7c5cff" />}
                </Space>
              ),
            },
            { key: "/settings", icon: <SettingOutlined />, label: "Cấu hình" },
          ]}
        />
      </Header>

      <Content className="rs-content">
        <div className="rs-container">
          <Routes>
            <Route path="/" element={<Navigate to="/stories" replace />} />
            <Route path="/stories" element={<Library />} />
            <Route path="/stories/:id" element={<StoryDetail />} />
            <Route path="/processes" element={<Processes />} />
            <Route path="/settings" element={<Settings />} />
          </Routes>
        </div>
      </Content>

      <Footer className="rs-footer">
        <Typography.Text type="secondary">
          Rewrite Story · viết lại truyện qua LLM chung của SenClaw
        </Typography.Text>
      </Footer>
    </Layout>
  );
}
