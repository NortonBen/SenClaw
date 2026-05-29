/**
 * Paginated browser for cognitive `DataPoint` nodes.
 *
 * Wraps the existing `/api/cognitive/nodes` endpoint (P12) — no new
 * backend work needed. Layout intentionally compact so the column scales
 * for graphs with thousands of nodes:
 *
 *   kind tag · name (or summary slice) · salience · last_seen · forget
 *
 * Clicking a row opens that node in the Graph view via the
 * `onOpenInGraph(nodeId)` callback so users can move from "list view" to
 * "neighborhood view" without losing the seed.
 */

import React, { useCallback, useEffect, useMemo, useState } from 'react';
import {
  Card,
  Empty,
  Pagination,
  Popconfirm,
  Select,
  Space,
  Table,
  Tag,
  Button,
  message,
  theme,
} from 'antd';
import {
  DeleteOutlined,
  NodeIndexOutlined,
  ReloadOutlined,
  ExperimentOutlined,
  ClearOutlined,
} from '@ant-design/icons';

interface NodeRow {
  id: string;
  kind: string;
  name: string;
  summary: string;
  salience: number;
  mention_count: number;
  created_at: number;
  last_seen_at: number;
}

interface ListResponse {
  total: number;
  nodes: NodeRow[];
}

interface Props {
  /** Tell the parent a node was deleted so it can refresh stats / search. */
  onMutated?: () => void;
  /** Hand off a node ID to the Graph section's seed. */
  onOpenInGraph?: (id: string) => void;
}

const PAGE_SIZE = 25;

export function DataPointsView({ onMutated, onOpenInGraph }: Props) {
  const { token } = theme.useToken();

  const [data, setData] = useState<ListResponse | null>(null);
  const [loading, setLoading] = useState(false);
  const [page, setPage] = useState(1);
  const [kindFilter, setKindFilter] = useState<string>('all');

  /** Fetch a page. The endpoint clamps limit to [1, 500] server-side. */
  const fetchPage = useCallback(async () => {
    setLoading(true);
    try {
      const params = new URLSearchParams({
        limit: String(PAGE_SIZE),
        offset: String((page - 1) * PAGE_SIZE),
      });
      if (kindFilter !== 'all') params.set('kind', kindFilter);
      const r = await fetch(`/api/cognitive/nodes?${params.toString()}`);
      if (!r.ok) throw new Error(await r.text());
      setData(await r.json());
    } catch (e: any) {
      message.error(`Load failed: ${e?.message ?? e}`);
    } finally {
      setLoading(false);
    }
  }, [page, kindFilter]);

  useEffect(() => {
    fetchPage();
  }, [fetchPage]);

  const forget = useCallback(
    async (id: string) => {
      try {
        const r = await fetch(`/api/cognitive/node/${id}`, { method: 'DELETE' });
        if (!r.ok) throw new Error(await r.text());
        message.success('forgotten');
        fetchPage();
        onMutated?.();
      } catch (e: any) {
        message.error(`Forget failed: ${e?.message ?? e}`);
      }
    },
    [fetchPage, onMutated],
  );

  /**
   * Re-run cognify on a chunk's text so back-filled chunks (saved when
   * the LLM was dormant) finally get entity/edge extraction. The endpoint
   * dedupes by content hash so the chunk node stays — only new edges
   * land in the graph.
   */
  /**
   * One-shot bulk cleanup. The backend removes envelope-wrapped chunks
   * (legacy junk from before the runtime sanitizer) and orphan entities
   * (entities left behind when their chunks were forgotten). Confirmed
   * via Popconfirm because the operation is destructive — we don't want
   * a misclick wiping data the user spent LLM cycles building.
   */
  const cleanupJunk = useCallback(async () => {
    try {
      // Run the full maintenance sweep — cleanup + merge — so duplicates
      // get folded onto a canonical entity in the same pass. Backed by the
      // same routine the periodic ticker runs.
      const r = await fetch('/api/cognitive/maintenance', { method: 'POST' });
      if (!r.ok) throw new Error(await r.text());
      const body = await r.json();
      const cleaned =
        (body.envelope_chunks_removed ?? 0) +
        (body.orphan_entities_removed ?? 0);
      const merged = body.entities_merged ?? 0;
      const inferred = body.associations_inferred ?? 0;
      if (cleaned === 0 && merged === 0 && inferred === 0) {
        message.success('Nothing to clean — your Data memory is already tidy.');
      } else {
        message.success(
          `Removed ${body.envelope_chunks_removed} envelope chunk(s) + ` +
            `${body.orphan_entities_removed} orphan entity(ies). ` +
            `Merged ${merged} duplicate entity(ies) across ` +
            `${body.groups_merged ?? 0} group(s). ` +
            `Inferred ${inferred} associative link(s).`,
        );
      }
      fetchPage();
      onMutated?.();
    } catch (e: any) {
      message.error(`Cleanup failed: ${e?.message ?? e}`);
    }
  }, [fetchPage, onMutated]);

  const reExtract = useCallback(
    async (id: string) => {
      try {
        const r = await fetch(`/api/cognitive/node/${id}/re-extract`, {
          method: 'POST',
        });
        if (!r.ok) throw new Error(await r.text());
        const body = await r.json();
        if (body.llm_skipped) {
          message.warning(
            'Cognitive LLM not configured — set Cognitive (or Main) Model in Settings.',
          );
        } else {
          message.success(
            `Extracted +${body.entities_added} entity, +${body.edges_added} edge`,
          );
        }
        onMutated?.();
        fetchPage();
      } catch (e: any) {
        message.error(`Re-extract failed: ${e?.message ?? e}`);
      }
    },
    [fetchPage, onMutated],
  );

  const columns = useMemo(
    () => [
      {
        title: 'Kind',
        dataIndex: 'kind',
        width: 110,
        render: (k: string) => <Tag>{k}</Tag>,
      },
      {
        title: 'Name / Summary',
        render: (_: unknown, row: NodeRow) => (
          <div style={{ minWidth: 0 }}>
            {row.name && (
              <div style={{ fontWeight: 600, fontSize: 13 }}>{row.name}</div>
            )}
            {row.summary && (
              <div
                style={{
                  color: token.colorTextSecondary,
                  fontSize: 12,
                  overflow: 'hidden',
                  textOverflow: 'ellipsis',
                  whiteSpace: 'nowrap',
                  maxWidth: 600,
                }}
              >
                {row.summary}
              </div>
            )}
            {!row.name && !row.summary && (
              <span style={{ color: token.colorTextQuaternary, fontSize: 11 }}>
                {row.id.slice(0, 8)}…
              </span>
            )}
          </div>
        ),
      },
      {
        title: 'Salience',
        dataIndex: 'salience',
        width: 100,
        render: (s: number) => s.toFixed(2),
      },
      {
        title: 'Mentions',
        dataIndex: 'mention_count',
        width: 90,
      },
      {
        title: 'Last seen',
        dataIndex: 'last_seen_at',
        width: 160,
        render: (ts: number) => new Date(ts * 1000).toLocaleString(),
      },
      {
        title: '',
        width: 160,
        render: (_: unknown, row: NodeRow) => (
          <Space size="small">
            <Button
              size="small"
              icon={<NodeIndexOutlined />}
              onClick={() => onOpenInGraph?.(row.id)}
              title="Open in Graph"
            />
            {row.kind === 'chunk' && (
              <Button
                size="small"
                icon={<ExperimentOutlined />}
                onClick={() => reExtract(row.id)}
                title="Re-run triplet extraction (useful when the chunk was saved before an LLM was configured)"
              />
            )}
            <Button
              size="small"
              danger
              icon={<DeleteOutlined />}
              onClick={() => forget(row.id)}
              title="Forget this node"
            />
          </Space>
        ),
      },
    ],
    [token, forget, onOpenInGraph],
  );

  return (
    <Card
      title="Data memory · DataPoints"
      size="small"
      extra={
        <Space>
          <Select
            size="small"
            value={kindFilter}
            onChange={v => {
              setPage(1);
              setKindFilter(v);
            }}
            style={{ width: 140 }}
            options={[
              { value: 'all', label: 'all kinds' },
              { value: 'entity', label: 'entity' },
              { value: 'chunk', label: 'chunk' },
              { value: 'summary', label: 'summary' },
              { value: 'custom', label: 'custom' },
            ]}
          />
          <Button
            size="small"
            icon={<ReloadOutlined />}
            onClick={fetchPage}
            loading={loading}
          >
            Refresh
          </Button>
          <Popconfirm
            title="Clean up junk?"
            description="Removes envelope-wrapped chunks and orphan entities. The good data stays. This cannot be undone."
            onConfirm={cleanupJunk}
            okText="Clean up"
            okButtonProps={{ danger: true }}
          >
            <Button size="small" icon={<ClearOutlined />}>
              Clean junk
            </Button>
          </Popconfirm>
        </Space>
      }
    >
      {!data || data.nodes.length === 0 ? (
        <Empty description="No nodes match this filter" />
      ) : (
        <>
          <Table
            rowKey="id"
            columns={columns as any}
            dataSource={data.nodes}
            pagination={false}
            size="small"
            loading={loading}
          />
          <div style={{ marginTop: 12, textAlign: 'right' }}>
            <Pagination
              size="small"
              current={page}
              pageSize={PAGE_SIZE}
              total={data.total}
              onChange={setPage}
              showSizeChanger={false}
              showTotal={total => `${total} nodes`}
            />
          </div>
        </>
      )}
    </Card>
  );
}
