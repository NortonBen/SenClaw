// App.tsx — the shell. A grouped sidebar drives which tool is active; the
// selected tool renders in ToolRunner. Theme (light/dark) is persisted. All
// compute happens in the backend or in offline libs — no router, no CDN.

import { useMemo, useState } from "react";
import {
  App as AntApp,
  ConfigProvider,
  Layout,
  Menu,
  Segmented,
  Typography,
  theme as antdTheme,
} from "antd";
import { BulbOutlined, MoonOutlined } from "@ant-design/icons";
import type { MenuProps } from "antd";

import ToolRunner from "./components/ToolRunner";
import { GROUPS, TOOLS } from "./tools";

const { Header, Sider, Content } = Layout;
const { Title } = Typography;

const THEME_KEY = "jt-theme";

function useTheme() {
  const [dark, setDark] = useState(() => localStorage.getItem(THEME_KEY) === "dark");
  const toggle = (d: boolean) => {
    setDark(d);
    localStorage.setItem(THEME_KEY, d ? "dark" : "light");
  };
  return { dark, toggle };
}

export default function App() {
  const { dark, toggle } = useTheme();
  const [active, setActive] = useState(TOOLS[0].key);
  const tool = useMemo(() => TOOLS.find((t) => t.key === active) ?? TOOLS[0], [active]);

  const menuItems: MenuProps["items"] = GROUPS.map((group) => ({
    key: group,
    label: group,
    type: "group",
    children: TOOLS.filter((t) => t.group === group).map((t) => ({
      key: t.key,
      label: t.label,
    })),
  }));

  return (
    <ConfigProvider
      theme={{
        algorithm: dark ? antdTheme.darkAlgorithm : antdTheme.defaultAlgorithm,
        token: { colorPrimary: "#6366f1", borderRadius: 8 },
      }}
    >
      <AntApp>
        <Layout style={{ minHeight: "100vh" }}>
          <Header
            style={{
              display: "flex",
              alignItems: "center",
              justifyContent: "space-between",
              paddingInline: 20,
            }}
          >
            <Title level={4} style={{ color: "#fff", margin: 0 }}>
              🧩 JSON Tools
            </Title>
            <Segmented
              value={dark ? "dark" : "light"}
              onChange={(v) => toggle(v === "dark")}
              options={[
                { label: "Sáng", value: "light", icon: <BulbOutlined /> },
                { label: "Tối", value: "dark", icon: <MoonOutlined /> },
              ]}
            />
          </Header>
          <Layout>
            <Sider
              width={240}
              breakpoint="lg"
              collapsedWidth={0}
              theme={dark ? "dark" : "light"}
              style={{ borderInlineEnd: "1px solid var(--jt-border)" }}
            >
              <Menu
                mode="inline"
                theme={dark ? "dark" : "light"}
                selectedKeys={[active]}
                onSelect={({ key }) => setActive(key)}
                items={menuItems}
                style={{ height: "100%", borderInlineEnd: 0, paddingBlock: 8 }}
              />
            </Sider>
            <Content style={{ padding: 24, overflow: "auto" }}>
              <ToolRunner key={tool.key} tool={tool} />
            </Content>
          </Layout>
        </Layout>
      </AntApp>
    </ConfigProvider>
  );
}
