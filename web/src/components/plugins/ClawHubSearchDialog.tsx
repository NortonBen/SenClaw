import { useEffect, useRef, useState } from 'react';
import {
  Modal,
  Input,
  List,
  Button,
  Typography,
  Tag,
  Empty,
  Spin,
  Alert,
  message,
  Space,
} from 'antd';
import { SearchOutlined, CloudDownloadOutlined, CheckOutlined } from '@ant-design/icons';

const { Text, Paragraph } = Typography;

interface RemoteResult {
  slug: string;
  displayName?: string;
  summary?: string | null;
  version?: string | null;
  score: number;
  installed: boolean;
}

interface Props {
  open: boolean;
  onClose: () => void;
  /** Called after a successful install so parent can refresh source/skill lists. */
  onInstalled?: (slug: string) => void;
}

async function apiFetch<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(path, init);
  if (!res.ok) {
    const text = await res.text();
    throw new Error(text || `${res.status} ${res.statusText}`);
  }
  return res.json();
}

/**
 * Search & install skills from clawhub.ai.
 *
 * Backend contract:
 *   - `GET  /api/skills/remote-search?q=<query>` → { results: RemoteResult[] }
 *   - `POST /api/skills/install { slug }`        → { ok, slug, version }
 *
 * The endpoints are already implemented in
 * `src/gateway/ui_server/skills.rs::skills_remote_search` / `skills_install`.
 */
export function ClawHubSearchDialog({ open, onClose, onInstalled }: Props) {
  const [query, setQuery] = useState('');
  const [results, setResults] = useState<RemoteResult[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');
  const [installingSlug, setInstallingSlug] = useState<string | null>(null);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Reset on open/close so each session starts fresh.
  useEffect(() => {
    if (!open) {
      setQuery('');
      setResults([]);
      setError('');
      return;
    }
  }, [open]);

  // Debounced remote search.
  useEffect(() => {
    if (!open) return;
    if (debounceRef.current) clearTimeout(debounceRef.current);
    if (!query.trim()) {
      setResults([]);
      setError('');
      return;
    }
    debounceRef.current = setTimeout(async () => {
      setLoading(true);
      setError('');
      try {
        const data = await apiFetch<{ results: RemoteResult[] }>(
          `/api/skills/remote-search?q=${encodeURIComponent(query)}`,
        );
        setResults(data.results ?? []);
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err));
        setResults([]);
      } finally {
        setLoading(false);
      }
    }, 400);
    return () => {
      if (debounceRef.current) clearTimeout(debounceRef.current);
    };
  }, [query, open]);

  const handleInstall = async (slug: string) => {
    setInstallingSlug(slug);
    try {
      await apiFetch('/api/skills/install', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ slug }),
      });
      message.success(`Installed ${slug}`);
      setResults((prev) =>
        prev.map((r) => (r.slug === slug ? { ...r, installed: true } : r)),
      );
      onInstalled?.(slug);
    } catch (err) {
      message.error(err instanceof Error ? err.message : 'Install failed');
    } finally {
      setInstallingSlug(null);
    }
  };

  return (
    <Modal
      title={
        <Space>
          <CloudDownloadOutlined />
          <span>Search ClaWHub Skills</span>
        </Space>
      }
      open={open}
      onCancel={onClose}
      footer={[
        <Text key="hint" type="secondary" style={{ float: 'left', fontSize: 12 }}>
          Powered by clawhub.ai
        </Text>,
        <Button key="close" onClick={onClose}>
          Close
        </Button>,
      ]}
      width={720}
      destroyOnClose
    >
      <Input
        prefix={<SearchOutlined />}
        placeholder="Search skills on clawhub.ai…"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        allowClear
        autoFocus
        size="large"
      />

      <div style={{ marginTop: 16, minHeight: 320, maxHeight: 480, overflow: 'auto' }}>
        {loading && (
          <div style={{ textAlign: 'center', padding: 48 }}>
            <Spin />
          </div>
        )}

        {!loading && error && (
          <Alert
            type="error"
            message="Search failed"
            description={error}
            showIcon
            style={{ marginTop: 8 }}
          />
        )}

        {!loading && !error && query.trim() && results.length === 0 && (
          <Empty description={`No results for "${query}"`} style={{ paddingTop: 48 }} />
        )}

        {!loading && !query.trim() && (
          <Empty
            description="Type a keyword to search ClaWHub"
            style={{ paddingTop: 48 }}
          />
        )}

        {!loading && results.length > 0 && (
          <List
            itemLayout="vertical"
            size="small"
            dataSource={results}
            renderItem={(r) => (
              <List.Item
                key={r.slug}
                actions={[
                  r.installed ? (
                    <Tag color="green" icon={<CheckOutlined />}>
                      Installed
                    </Tag>
                  ) : (
                    <Button
                      type="primary"
                      icon={<CloudDownloadOutlined />}
                      loading={installingSlug === r.slug}
                      onClick={() => handleInstall(r.slug)}
                      size="small"
                    >
                      Install
                    </Button>
                  ),
                ]}
              >
                <List.Item.Meta
                  title={
                    <Space>
                      <Text strong>{r.displayName || r.slug}</Text>
                      {r.version && <Tag color="blue">{r.version}</Tag>}
                      <Text type="secondary" style={{ fontSize: 12 }}>
                        {r.slug}
                      </Text>
                    </Space>
                  }
                  description={
                    r.summary ? (
                      <Paragraph ellipsis={{ rows: 2 }} style={{ margin: 0 }}>
                        {r.summary}
                      </Paragraph>
                    ) : (
                      <Text type="secondary">No description</Text>
                    )
                  }
                />
              </List.Item>
            )}
          />
        )}
      </div>
    </Modal>
  );
}
