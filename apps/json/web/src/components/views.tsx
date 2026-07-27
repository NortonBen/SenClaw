// views.tsx — custom result renderers for tools whose output is richer than a
// single text blob: the JSON tree, the stats summary, and the structural diff.

import { useMemo } from "react";
import { Descriptions, Empty, Table, Tag, Tree, Typography } from "antd";
import type { DataNode } from "antd/es/tree";

const { Text } = Typography;

// ── JSON tree ───────────────────────────────────────────────────────────────

function valueLabel(v: unknown): { text: string; color: string } {
  if (v === null) return { text: "null", color: "#999" };
  switch (typeof v) {
    case "string":
      return { text: `"${v}"`, color: "#16a34a" };
    case "number":
      return { text: String(v), color: "#2563eb" };
    case "boolean":
      return { text: String(v), color: "#c026d3" };
    default:
      return { text: "", color: "" };
  }
}

function toNodes(value: unknown, keyPrefix: string, label: string): DataNode {
  if (Array.isArray(value)) {
    return {
      key: keyPrefix,
      title: (
        <span>
          <Text strong>{label}</Text> <Text type="secondary">[{value.length}]</Text>
        </span>
      ),
      children: value.map((child, i) => toNodes(child, `${keyPrefix}.${i}`, String(i))),
    };
  }
  if (value && typeof value === "object") {
    const entries = Object.entries(value as Record<string, unknown>);
    return {
      key: keyPrefix,
      title: (
        <span>
          <Text strong>{label}</Text>{" "}
          <Text type="secondary">{`{${entries.length}}`}</Text>
        </span>
      ),
      children: entries.map(([k, child]) => toNodes(child, `${keyPrefix}.${k}`, k)),
    };
  }
  const { text, color } = valueLabel(value);
  return {
    key: keyPrefix,
    title: (
      <span>
        <Text strong>{label}</Text>: <span style={{ color }}>{text}</span>
      </span>
    ),
  };
}

export function JsonTree({ source }: { source: string }) {
  const parsed = useMemo(() => {
    try {
      return { value: JSON.parse(source), error: null as string | null };
    } catch (e) {
      return { value: null, error: e instanceof Error ? e.message : String(e) };
    }
  }, [source]);

  if (parsed.error) return <Empty description={`Không parse được: ${parsed.error}`} />;
  const root = toNodes(parsed.value, "$", "$");
  return (
    <Tree
      showLine
      defaultExpandAll
      selectable={false}
      treeData={[root]}
      style={{ padding: 8 }}
    />
  );
}

// ── stats ─────────────────────────────────────────────────────────────────

export function StatsView({ data }: { data: Record<string, unknown> }) {
  const counts = (data.counts ?? {}) as Record<string, number>;
  const keys = (data.object_keys ?? {}) as Record<string, number>;
  return (
    <Descriptions bordered size="small" column={2} style={{ margin: 8 }}>
      <Descriptions.Item label="Kiểu gốc">{String(data.root_type)}</Descriptions.Item>
      <Descriptions.Item label="Độ sâu tối đa">{String(data.max_depth)}</Descriptions.Item>
      <Descriptions.Item label="Số node">{String(data.nodes)}</Descriptions.Item>
      <Descriptions.Item label="Bytes">{String(data.bytes)}</Descriptions.Item>
      <Descriptions.Item label="Dòng">{String(data.lines)}</Descriptions.Item>
      <Descriptions.Item label="Khoá (tổng / duy nhất)">
        {String(keys.total ?? 0)} / {String(keys.unique ?? 0)}
      </Descriptions.Item>
      <Descriptions.Item label="Objects">{String(counts.objects ?? 0)}</Descriptions.Item>
      <Descriptions.Item label="Arrays">{String(counts.arrays ?? 0)}</Descriptions.Item>
      <Descriptions.Item label="Strings">{String(counts.strings ?? 0)}</Descriptions.Item>
      <Descriptions.Item label="Numbers">{String(counts.numbers ?? 0)}</Descriptions.Item>
      <Descriptions.Item label="Booleans">{String(counts.booleans ?? 0)}</Descriptions.Item>
      <Descriptions.Item label="Nulls">{String(counts.nulls ?? 0)}</Descriptions.Item>
    </Descriptions>
  );
}

// ── diff ────────────────────────────────────────────────────────────────────

type Change = {
  path: string;
  op: "added" | "removed" | "changed";
  left?: unknown;
  right?: unknown;
};

const OP_COLOR: Record<Change["op"], string> = {
  added: "green",
  removed: "red",
  changed: "gold",
};

function cell(v: unknown): string {
  if (v === undefined) return "—";
  return typeof v === "string" ? v : JSON.stringify(v);
}

export function DiffView({ data }: { data: Record<string, unknown> }) {
  const changes = (data.changes ?? []) as Change[];
  if (data.equal) return <Empty description="Hai tài liệu giống hệt nhau" />;
  return (
    <Table<Change>
      size="small"
      rowKey={(r) => r.path + r.op}
      dataSource={changes}
      pagination={changes.length > 50 ? { pageSize: 50 } : false}
      style={{ margin: 8 }}
      columns={[
        {
          title: "Thao tác",
          dataIndex: "op",
          width: 110,
          render: (op: Change["op"]) => <Tag color={OP_COLOR[op]}>{op}</Tag>,
        },
        { title: "Đường dẫn", dataIndex: "path", width: 240, ellipsis: true },
        {
          title: "Trái",
          dataIndex: "left",
          render: (v) => <Text code>{cell(v)}</Text>,
        },
        {
          title: "Phải",
          dataIndex: "right",
          render: (v) => <Text code>{cell(v)}</Text>,
        },
      ]}
    />
  );
}
