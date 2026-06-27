import { useEffect, useState } from 'react';
import { Alert, Button, Drawer, Input, InputNumber, Space, Spin, Tag, Typography, theme, App as AntApp } from 'antd';
import { RobotOutlined, UndoOutlined } from '@ant-design/icons';
import { api, type LlmInfo, type Settings } from '../api';

const { Text, Title, Paragraph } = Typography;
const { TextArea } = Input;

interface Props {
  open: boolean;
  onClose: () => void;
  onSaved: () => void;
}

export function SettingsDrawer({ open, onClose, onSaved }: Props) {
  const { token } = theme.useToken();
  const { message } = AntApp.useApp();
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [factory, setFactory] = useState<string[]>([]);
  const [defaults, setDefaults] = useState('');
  const [custom, setCustom] = useState('');
  const [minified, setMinified] = useState(2000);
  const [llm, setLlm] = useState<LlmInfo | null>(null);

  useEffect(() => {
    if (!open) return;
    setLoading(true);
    Promise.all([api.getSettings(), api.llmInfo().catch(() => null)])
      .then(([s, l]: [Settings, LlmInfo | null]) => {
        setFactory(s.factoryExcludes);
        setDefaults(s.defaultExcludes.join('\n'));
        setCustom(s.customExcludes.join('\n'));
        setMinified(s.minifiedMaxLine);
        setLlm(l);
      })
      .catch((e) => message.error((e as Error).message))
      .finally(() => setLoading(false));
  }, [open, message]);

  const lines = (s: string) => s.split('\n').map((x) => x.trim()).filter(Boolean);

  const save = async () => {
    setSaving(true);
    try {
      await api.saveSettings({
        defaultExcludes: lines(defaults),
        customExcludes: lines(custom),
        minifiedMaxLine: minified,
      });
      message.success('Đã lưu cài đặt. Bấm Index lại để áp dụng bộ lọc mới.');
      onSaved();
      onClose();
    } catch (e) {
      message.error((e as Error).message);
    } finally {
      setSaving(false);
    }
  };

  return (
    <Drawer
      title="Cài đặt DeepWiki"
      open={open}
      onClose={onClose}
      width={460}
      extra={<Button type="primary" loading={saving} onClick={save}>Lưu</Button>}
    >
      {loading ? (
        <div style={{ textAlign: 'center', padding: 40 }}><Spin /></div>
      ) : (
        <Space direction="vertical" size="large" style={{ width: '100%' }}>
          {/* LLM */}
          <div>
            <Title level={5} style={{ marginTop: 0 }}><RobotOutlined /> LLM (Hỏi AI)</Title>
            {llm?.ok ? (
              <Alert
                type="success"
                showIcon
                message={<span>Dùng <b>Model Main</b> của SenClaw</span>}
                description={
                  <Space direction="vertical" size={2}>
                    <span><Tag color="blue">{llm.model}</Tag> <Text type="secondary">{llm.provider}</Text></span>
                    <Text type="secondary" style={{ fontSize: 12 }}>daemon: {llm.daemon}</Text>
                  </Space>
                }
              />
            ) : (
              <Alert
                type="warning"
                showIcon
                message="Chưa lấy được model từ SenClaw"
                description={
                  <Text type="secondary" style={{ fontSize: 12 }}>
                    {llm?.error ?? 'Daemon không phản hồi.'} Hỏi AI cần daemon SenClaw (build từ repo này, đã bật bridge) +
                    một Model active trong Settings → Models. DeepWiki luôn dùng <b>Model Main</b> đang active.
                  </Text>
                }
              />
            )}
          </div>

          {/* Default excludes */}
          <div>
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 6 }}>
              <Title level={5} style={{ margin: 0 }}>Loại trừ mặc định</Title>
              <Button size="small" icon={<UndoOutlined />} onClick={() => setDefaults(factory.join('\n'))}>
                Khôi phục mặc định
              </Button>
            </div>
            <Paragraph type="secondary" style={{ fontSize: 12, margin: '0 0 6px' }}>
              Folder/file bỏ qua khi index (mỗi dòng 1 glob). Tên trống = thư mục bất kỳ tên đó.
            </Paragraph>
            <TextArea value={defaults} onChange={(e) => setDefaults(e.target.value)} autoSize={{ minRows: 6, maxRows: 14 }}
              style={{ fontFamily: 'ui-monospace, monospace', fontSize: 12.5 }} />
          </div>

          {/* Custom excludes */}
          <div>
            <Title level={5} style={{ margin: '0 0 6px' }}>Loại trừ thêm (của bạn)</Title>
            <TextArea value={custom} onChange={(e) => setCustom(e.target.value)} autoSize={{ minRows: 2, maxRows: 8 }}
              placeholder={'vd:\nfixtures\n*.test.ts'}
              style={{ fontFamily: 'ui-monospace, monospace', fontSize: 12.5 }} />
          </div>

          {/* Minified threshold */}
          <div>
            <Title level={5} style={{ margin: '0 0 6px' }}>Ngưỡng minified</Title>
            <Space>
              <InputNumber value={minified} min={200} max={20000} step={500} onChange={(v) => setMinified(v ?? 2000)} style={{ width: 140 }} />
              <Text type="secondary" style={{ fontSize: 12 }}>ký tự/dòng — file vượt ngưỡng bị coi là sinh tự động và bỏ qua</Text>
            </Space>
          </div>

          <Text type="secondary" style={{ fontSize: 12, color: token.colorTextTertiary }}>
            Sau khi lưu, bấm <b>Index</b> để áp dụng. File đã loại sẽ bị gỡ khỏi index.
          </Text>
        </Space>
      )}
    </Drawer>
  );
}
