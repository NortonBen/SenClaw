import React, { useCallback, useEffect, useMemo, useState } from 'react';
import {
  Typography,
  Card,
  Space,
  Input,
  Button,
  Select,
  Table,
  Tag,
  message,
  Spin,
  Alert,
  Tabs,
  theme,
} from 'antd';
import {
  IdcardOutlined,
  LockOutlined,
  GlobalOutlined,
  ToolOutlined,
  FileTextOutlined,
  ReloadOutlined,
} from '@ant-design/icons';

const { Title, Text, Paragraph } = Typography;

type Tier = 'public' | 'private';

interface Field {
  key: string;
  value: string;
  tier: Tier;
}

interface Directive {
  text: string;
  observed: string;
  status: 'active' | 'superseded';
  tier: Tier;
}

interface UserProfileDto {
  fields: Field[];
  directives: Directive[];
  notes: string;
  path: string;
  preview_full: string | null;
  preview_public: string | null;
}

/**
 * Fields the form always offers, in the order they read as a sentence about a
 * person. The file may hold others (hand-added, or set by the agent); those
 * are appended below rather than dropped, because a save that silently
 * deletes what it did not recognise is worse than an unfamiliar row.
 */
const KNOWN_FIELDS: { key: string; label: string; placeholder: string }[] = [
  { key: 'name', label: 'Họ tên', placeholder: 'Nguyễn Văn A' },
  { key: 'preferred_name', label: 'Xưng hô', placeholder: 'anh A' },
  { key: 'pronouns', label: 'Đại từ', placeholder: 'anh ấy / cô ấy' },
  { key: 'language', label: 'Ngôn ngữ', placeholder: 'vi' },
  { key: 'timezone', label: 'Múi giờ', placeholder: 'Asia/Ho_Chi_Minh' },
  { key: 'occupation', label: 'Nghề nghiệp', placeholder: 'Backend engineer' },
  { key: 'email', label: 'Email', placeholder: 'a@example.com' },
  { key: 'location', label: 'Địa điểm', placeholder: 'Hà Nội, Việt Nam' },
  { key: 'phone', label: 'Điện thoại', placeholder: '09xx xxx xxx' },
];

/** Mirrors `DEFAULT_PUBLIC_FIELDS` in `src/user_profile/parse.rs`. */
const DEFAULT_PUBLIC = new Set([
  'name',
  'preferred_name',
  'pronouns',
  'language',
  'timezone',
  'occupation',
]);

export const UserProfileSettings: React.FC = () => {
  const { token } = theme.useToken();
  const [dto, setDto] = useState<UserProfileDto | null>(null);
  const [fields, setFields] = useState<Record<string, Field>>({});
  const [notes, setNotes] = useState('');
  const [tools, setTools] = useState('');
  const [rules, setRules] = useState('');
  const [paths, setPaths] = useState<{ tools: string; rules: string }>({ tools: '', rules: '' });
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);

  const load = useCallback(async () => {
    try {
      const [p, t, r] = await Promise.all([
        fetch('/api/user-profile').then((x) => x.json() as Promise<UserProfileDto>),
        fetch('/api/tools-notes').then((x) => x.json()),
        fetch('/api/agents-rules').then((x) => x.json()),
      ]);
      setDto(p);
      const map: Record<string, Field> = {};
      for (const f of p.fields) map[f.key] = f;
      for (const k of KNOWN_FIELDS) {
        if (!map[k.key]) {
          map[k.key] = {
            key: k.key,
            value: '',
            tier: DEFAULT_PUBLIC.has(k.key) ? 'public' : 'private',
          };
        }
      }
      setFields(map);
      setNotes(p.notes);
      setTools(t.content ?? '');
      setRules(r.content ?? '');
      setPaths({ tools: t.path ?? '', rules: r.path ?? '' });
    } catch (e) {
      console.error('Failed to load user profile:', e);
      message.error('Không tải được hồ sơ người dùng');
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  /// Re-fetch and re-seed. `load` overwrites the field map wholesale, so any
  /// unsaved edit is discarded — which is what a reload button means.
  const reload = useCallback(() => {
    setLoading(true);
    void load();
  }, [load]);

  const saveProfile = async () => {
    setSaving(true);
    try {
      const r = await fetch('/api/user-profile', {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ fields: Object.values(fields), notes }),
      });
      if (!r.ok) throw new Error(String(r.status));
      setDto(await r.json());
      message.success('Đã lưu hồ sơ — agent sẽ dùng từ phiên chat tiếp theo');
    } catch (e) {
      message.error('Lưu hồ sơ thất bại');
    } finally {
      setSaving(false);
    }
  };

  const saveFlat = async (which: 'tools-notes' | 'agents-rules', content: string) => {
    setSaving(true);
    try {
      const r = await fetch(`/api/${which}`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ content }),
      });
      if (!r.ok) throw new Error(String(r.status));
      message.success('Đã lưu');
    } catch (e) {
      message.error('Lưu thất bại');
    } finally {
      setSaving(false);
    }
  };

  const extraFields = useMemo(
    () => Object.values(fields).filter((f) => !KNOWN_FIELDS.some((k) => k.key === f.key)),
    [fields]
  );

  const setField = (key: string, patch: Partial<Field>) =>
    setFields((prev) => ({ ...prev, [key]: { ...prev[key], key, ...patch } as Field }));

  if (loading) {
    return (
      <div style={{ textAlign: 'center', padding: 48 }}>
        <Spin />
      </div>
    );
  }

  const tierSelect = (f: Field) => (
    <Select<Tier>
      value={f.tier}
      style={{ width: 132 }}
      onChange={(tier) => setField(f.key, { tier })}
      options={[
        {
          value: 'public',
          label: (
            <span>
              <GlobalOutlined /> Công khai
            </span>
          ),
        },
        {
          value: 'private',
          label: (
            <span>
              <LockOutlined /> Riêng tư
            </span>
          ),
        },
      ]}
    />
  );

  const fieldRow = (key: string, label: string, placeholder: string) => {
    const f = fields[key];
    if (!f) return null;
    return (
      <Space key={key} style={{ width: '100%' }} align="start">
        <div style={{ width: 120, paddingTop: 5 }}>
          <Text type="secondary">{label}</Text>
        </div>
        <Input
          style={{ width: 320 }}
          value={f.value}
          placeholder={placeholder}
          onChange={(e) => setField(key, { value: e.target.value })}
        />
        {tierSelect(f)}
      </Space>
    );
  };

  const profileTab = (
    <Space direction="vertical" size="large" style={{ width: '100%' }}>
      <Alert
        type="info"
        showIcon
        message="Công khai vs riêng tư"
        description={
          <>
            Trường <b>công khai</b> đi vào mọi ngữ cảnh, kể cả nhóm chat trên Telegram/Feishu.
            Trường <b>riêng tư</b> chỉ xuất hiện trong hội thoại 1-1 của chính bạn. Email, địa
            chỉ và số điện thoại mặc định riêng tư.
          </>
        }
      />

      <Card size="small" title="Thông tin">
        <Space direction="vertical" size="middle" style={{ width: '100%' }}>
          {KNOWN_FIELDS.map((k) => fieldRow(k.key, k.label, k.placeholder))}
          {extraFields.length > 0 && (
            <>
              <Text type="secondary" style={{ fontSize: 12 }}>
                Trường thêm tay hoặc do agent ghi:
              </Text>
              {extraFields.map((f) => fieldRow(f.key, f.key, ''))}
            </>
          )}
        </Space>
      </Card>

      <Card size="small" title="Ghi chú thêm">
        <Input.TextArea
          rows={4}
          value={notes}
          onChange={(e) => setNotes(e.target.value)}
          placeholder="Bối cảnh tự do: đang làm dự án gì, quan tâm điều gì…"
        />
      </Card>

      <Card
        size="small"
        title="Quy tắc agent đã học"
        extra={
          <Text type="secondary" style={{ fontSize: 12 }}>
            Agent tự ghi qua tool <code>profile_update</code>
          </Text>
        }
      >
        {dto && dto.directives.length > 0 ? (
          <Table<Directive>
            size="small"
            pagination={false}
            rowKey={(d, i) => `${d.observed}-${i}`}
            dataSource={dto.directives}
            columns={[
              { title: 'Quy tắc', dataIndex: 'text' },
              { title: 'Ghi nhận', dataIndex: 'observed', width: 110 },
              {
                title: 'Trạng thái',
                dataIndex: 'status',
                width: 120,
                render: (s: string) =>
                  s === 'active' ? (
                    <Tag color="green">đang áp dụng</Tag>
                  ) : (
                    <Tag>đã thay thế</Tag>
                  ),
              },
              {
                title: 'Phạm vi',
                dataIndex: 'tier',
                width: 110,
                render: (t: Tier) =>
                  t === 'public' ? <Tag color="blue">công khai</Tag> : <Tag color="orange">riêng tư</Tag>,
              },
            ]}
          />
        ) : (
          <Text type="secondary">
            Chưa có. Nói với agent "từ giờ trả lời ngắn gọn" và nó sẽ ghi vào đây.
          </Text>
        )}
      </Card>

      {/* The two previews are the only way the tier rule is visible. Without
          them the user has to take on faith that a group chat cannot see
          their address. */}
      <Card size="small" title="Agent thực sự nhận được gì">
        <Space direction="vertical" size="middle" style={{ width: '100%' }}>
          <div>
            <Text strong>
              <LockOutlined /> Hội thoại riêng tư
            </Text>
            <pre
              style={{
                background: token.colorFillQuaternary,
                padding: 12,
                borderRadius: 6,
                whiteSpace: 'pre-wrap',
                marginTop: 8,
                fontSize: 12,
              }}
            >
              {dto?.preview_full ?? '(không có gì)'}
            </pre>
          </div>
          <div>
            <Text strong>
              <GlobalOutlined /> Nhóm chat
            </Text>
            <pre
              style={{
                background: token.colorFillQuaternary,
                padding: 12,
                borderRadius: 6,
                whiteSpace: 'pre-wrap',
                marginTop: 8,
                fontSize: 12,
              }}
            >
              {dto?.preview_public ?? '(không có gì)'}
            </pre>
          </div>
        </Space>
      </Card>

      <Space>
        <Button type="primary" loading={saving} onClick={saveProfile}>
          Lưu hồ sơ
        </Button>
        <Text type="secondary" style={{ fontSize: 12 }}>
          <code>{dto?.path}</code>
        </Text>
      </Space>
    </Space>
  );

  const flatTab = (
    which: 'tools-notes' | 'agents-rules',
    value: string,
    setValue: (v: string) => void,
    path: string,
    description: React.ReactNode
  ) => (
    <Space direction="vertical" size="large" style={{ width: '100%' }}>
      <Alert type="info" showIcon message={description} />
      <Input.TextArea
        rows={18}
        value={value}
        onChange={(e) => setValue(e.target.value)}
        style={{ fontFamily: 'monospace', fontSize: 13 }}
      />
      <Space>
        <Button type="primary" loading={saving} onClick={() => void saveFlat(which, value)}>
          Lưu
        </Button>
        <Text type="secondary" style={{ fontSize: 12 }}>
          <code>{path}</code>
        </Text>
      </Space>
    </Space>
  );

  return (
    <div>
      <Space style={{ width: '100%', justifyContent: 'space-between' }} align="start">
        <Title level={4} style={{ marginBottom: 0 }}>
          <IdcardOutlined /> Hồ sơ người dùng
        </Title>
        {/* This screen is not the only writer — the agent edits the same
            profile via `profile_update` mid-chat — so a stale form would
            overwrite those changes on the next save. */}
        <Button icon={<ReloadOutlined />} loading={loading} onClick={reload}>
          Tải lại
        </Button>
      </Space>
      <Paragraph type="secondary">
        Agent đọc những file này để biết <b>bạn</b> là ai — khác với Persona (SOUL.md), thứ mô tả
        agent là ai. Dùng chung cho mọi profile agent, không phải khai lại từng cái.
      </Paragraph>

      <Tabs
        items={[
          { key: 'profile', label: 'USER.md — Hồ sơ', icon: <IdcardOutlined />, children: profileTab },
          {
            key: 'tools',
            label: 'TOOLS.md — Môi trường',
            icon: <ToolOutlined />,
            children: flatTab(
              'tools-notes',
              tools,
              setTools,
              paths.tools,
              <>
                Ghi chú riêng của máy này: tên/IP máy chủ SSH, tên camera, giọng đọc TTS ưa dùng.
                Để tách khỏi skill thì skill vẫn chia sẻ được mà không lộ hạ tầng.{' '}
                <b>Chỉ hiện trong hội thoại riêng tư.</b>
              </>
            ),
          },
          {
            key: 'rules',
            label: 'AGENTS.md — Quy tắc',
            icon: <FileTextOutlined />,
            children: flatTab(
              'agents-rules',
              rules,
              setRules,
              paths.rules,
              <>
                Quy tắc vận hành áp dụng cho mọi phiên, nối vào cuối system prompt. Phần Safety
                mặc định của SenClaw luôn thắng nếu có mâu thuẫn — nội dung ở đây không ghi đè
                được nó.
              </>
            ),
          },
        ]}
      />
    </div>
  );
};
