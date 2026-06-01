import React, { useCallback, useEffect, useRef, useState } from 'react';
import {
  Typography,
  Table,
  Tag,
  Button,
  Space,
  Progress,
  Popconfirm,
  message,
  Alert,
  Card,
  Select,
  Spin,
  Input,
  InputNumber,
  Row,
  Col,
} from 'antd';
import {
  CloudDownloadOutlined,
  DeleteOutlined,
  StopOutlined,
  ReloadOutlined,
  CheckCircleOutlined,
  AudioOutlined,
  PlayCircleOutlined,
} from '@ant-design/icons';

const { Title, Text, Paragraph } = Typography;

type DownloadStatus = 'queued' | 'listing' | 'downloading' | 'done' | 'error' | 'cancelled';

interface DownloadState {
  model_id: string;
  status: DownloadStatus;
  total_bytes: number;
  downloaded_bytes: number;
  current_file: string | null;
  files_total: number;
  files_done: number;
  error: string | null;
}

interface TtsModel {
  id: string;
  label: string;
  approx_size_gb: number;
  default_language: string;
  languages: string[];
  description: string;
  installed: boolean;
  on_disk_path: string;
  custom: boolean;
  download: DownloadState | null;
}

const LANGUAGES: { code: string; label: string }[] = [
  { code: 'vi', label: 'Tiếng Việt' },
  { code: 'en', label: 'English' },
];

function fmtBytes(n: number): string {
  if (!n) return '0 B';
  const u = ['B', 'KB', 'MB', 'GB'];
  const i = Math.min(Math.floor(Math.log(n) / Math.log(1024)), u.length - 1);
  return `${(n / 1024 ** i).toFixed(1)} ${u[i]}`;
}

export const TtsSettings: React.FC = () => {
  const [models, setModels] = useState<TtsModel[]>([]);
  const [loading, setLoading] = useState(true);
  const [hfInput, setHfInput] = useState('');
  const pollRef = useRef<number | null>(null);

  const [activeModel, setActiveModel] = useState<string>('macos-speech');
  const [voice, setVoice] = useState<string>('Linh');
  const [language, setLanguage] = useState<string>('vi');
  const [speed, setSpeed] = useState<number>(1.0);
  
  const [testText, setTestText] = useState('Chào bạn, đây là giọng nói thử nghiệm.');
  const [synthesizing, setSynthesizing] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const [listRes, setRes] = await Promise.all([
        fetch('/api/tts/models'),
        fetch('/api/tts/settings'),
      ]);
      if (!listRes.ok) throw new Error(`list failed (${listRes.status})`);
      const list = await listRes.json();
      setModels(list.models || []);
      if (setRes.ok) {
        const s = await setRes.json();
        if (s.model_id) setActiveModel(s.model_id);
        if (s.voice) setVoice(s.voice);
        if (s.language) setLanguage(s.language);
        if (s.speed) setSpeed(s.speed);
      }
    } catch (e: any) {
      message.error(`Không tải được danh sách TTS: ${e.message}`);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  useEffect(() => {
    const active = models.some(
      (m) =>
        m.download &&
        ['queued', 'listing', 'downloading'].includes(m.download.status)
    );
    if (active && pollRef.current === null) {
      pollRef.current = window.setInterval(refresh, 1500);
    } else if (!active && pollRef.current !== null) {
      window.clearInterval(pollRef.current);
      pollRef.current = null;
    }
    return () => {
      if (pollRef.current !== null) {
        window.clearInterval(pollRef.current);
        pollRef.current = null;
      }
    };
  }, [models, refresh]);

  const download = async (id: string) => {
    const res = await fetch(`/api/tts/models/${encodeURIComponent(id)}/download`, {
      method: 'POST',
    });
    if (!res.ok) {
      message.error(`Tải thất bại: ${await res.text()}`);
      return;
    }
    message.success(`Bắt đầu tải ${id}`);
    refresh();
  };

  const cancel = async (id: string) => {
    await fetch(`/api/tts/models/${encodeURIComponent(id)}/cancel`, { method: 'POST' });
    refresh();
  };

  const remove = async (id: string) => {
    await fetch(`/api/tts/models/${encodeURIComponent(id)}`, { method: 'DELETE' });
    message.success('Đã xoá model');
    refresh();
  };

  const saveSettings = async () => {
    const body = { model_id: activeModel, voice, language, speed };
    const res = await fetch('/api/tts/settings', {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    });
    if (res.ok) message.success('Đã lưu cấu hình TTS');
    else message.error(`Lưu cấu hình thất bại: ${await res.text()}`);
  };

  const synthesizeTest = async () => {
    if (!testText.trim()) return;
    setSynthesizing(true);
    try {
      const body = { text: testText, model_id: activeModel, voice, language, speed };
      const res = await fetch('/api/tts/synthesize', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      });
      if (!res.ok) {
        message.error(`Lỗi tạo giọng nói: ${await res.text()}`);
        return;
      }
      const blob = await res.blob();
      const url = URL.createObjectURL(blob);
      const audio = new Audio(url);
      audio.onended = () => URL.revokeObjectURL(url);
      audio.play();
    } catch (e: any) {
      message.error(`Lỗi phát audio: ${e.message}`);
    } finally {
      setSynthesizing(false);
    }
  };

  const installedModels = models.filter((m) => m.installed);

  const columns = [
    {
      title: 'Model',
      dataIndex: 'label',
      render: (label: string, m: TtsModel) => (
        <div>
          <div style={{ fontWeight: 600 }}>{label}</div>
          <Text type="secondary" style={{ fontSize: 12 }}>
            {m.id} {m.approx_size_gb > 0 ? `· ~${m.approx_size_gb} GB` : ''}
          </Text>
          {m.description && <div style={{ fontSize: 12, marginTop: 4, color: '#888' }}>{m.description}</div>}
        </div>
      ),
    },
    {
      title: 'Trạng thái',
      key: 'status',
      width: 280,
      render: (_: any, m: TtsModel) => {
        const d = m.download;
        if (d && ['queued', 'listing', 'downloading'].includes(d.status)) {
          const pct = d.total_bytes
            ? Math.round((d.downloaded_bytes / d.total_bytes) * 100)
            : 0;
          return (
            <div style={{ minWidth: 240 }}>
              <Progress percent={pct} size="small" status="active" />
              <Text type="secondary" style={{ fontSize: 12 }}>
                {d.files_done}/{d.files_total} tệp · {fmtBytes(d.downloaded_bytes)}
                {d.current_file ? ` · ${d.current_file}` : ''}
              </Text>
            </div>
          );
        }
        if (d && d.status === 'error') {
          return <Tag color="error">Lỗi: {d.error}</Tag>;
        }
        if (m.installed) {
          return (
            <Tag icon={<CheckCircleOutlined />} color="success">
              Đã cài
            </Tag>
          );
        }
        return <Tag>Chưa cài</Tag>;
      },
    },
    {
      title: '',
      key: 'actions',
      width: 160,
      render: (_: any, m: TtsModel) => {
        if (m.id === 'macos-speech') return <Tag color="blue">Native</Tag>;
        const d = m.download;
        if (d && ['queued', 'listing', 'downloading'].includes(d.status)) {
          return (
            <Button size="small" danger icon={<StopOutlined />} onClick={() => cancel(m.id)}>
              Huỷ
            </Button>
          );
        }
        return (
          <Space>
            {!m.installed && (
              <Button
                size="small"
                type="primary"
                icon={<CloudDownloadOutlined />}
                onClick={() => download(m.id)}
              >
                Tải
              </Button>
            )}
            {m.installed && (
              <Popconfirm title="Xoá model này?" onConfirm={() => remove(m.id)}>
                <Button size="small" danger icon={<DeleteOutlined />} />
              </Popconfirm>
            )}
          </Space>
        );
      },
    },
  ];

  return (
    <div>
      <Space style={{ width: '100%', justifyContent: 'space-between', marginBottom: 16 }}>
        <Title level={3} style={{ margin: 0 }}>
          Text-to-Speech (TTS)
        </Title>
        <Space>
          <Button type="primary" onClick={saveSettings}>
            Lưu cài đặt
          </Button>
          <Button icon={<ReloadOutlined />} onClick={refresh}>
            Làm mới
          </Button>
        </Space>
      </Space>
      <Paragraph type="secondary">
        Quản lý và cấu hình các model đọc văn bản (Text-to-Speech). Hệ thống hỗ trợ tốt nhất giọng đọc Native của macOS (chạy nhanh, không độ trễ, hoàn toàn offline).
      </Paragraph>

      <Table
        rowKey="id"
        size="small"
        columns={columns as any}
        dataSource={models}
        loading={loading}
        pagination={false}
        style={{ marginBottom: 16 }}
      />

      <Card size="small" title="Cấu hình giọng đọc mặc định" style={{ marginBottom: 16 }}>
        <Row gutter={[16, 16]}>
          <Col xs={24} md={12}>
            <Text type="secondary" style={{ display: 'block', marginBottom: 4 }}>
              Model TTS:
            </Text>
            <Select
              style={{ width: '100%' }}
              value={activeModel}
              onChange={(v) => setActiveModel(v)}
              options={installedModels.map((m) => ({ value: m.id, label: m.label }))}
            />
          </Col>
          <Col xs={24} md={12}>
            <Text type="secondary" style={{ display: 'block', marginBottom: 4 }}>
              Ngôn ngữ:
            </Text>
            <Select
              style={{ width: '100%' }}
              value={language}
              onChange={(v) => setLanguage(v)}
              options={LANGUAGES.map((l) => ({ value: l.code, label: l.label }))}
            />
          </Col>
          <Col xs={24} md={12}>
            <Text type="secondary" style={{ display: 'block', marginBottom: 4 }}>
              Giọng đọc (Voice):
            </Text>
            <Input 
              value={voice} 
              onChange={(e) => setVoice(e.target.value)} 
              placeholder="Ví dụ: Linh (VN), Samantha (EN)..."
            />
            <Text type="secondary" style={{ fontSize: 12 }}>
              macOS mặc định: Linh (tiếng Việt), Samantha (tiếng Anh)
            </Text>
          </Col>
          <Col xs={24} md={12}>
            <Text type="secondary" style={{ display: 'block', marginBottom: 4 }}>
              Tốc độ đọc (Speed):
            </Text>
            <InputNumber
              style={{ width: '100%' }}
              min={0.25}
              max={4.0}
              step={0.1}
              value={speed}
              onChange={(v) => setSpeed(v || 1.0)}
            />
          </Col>
        </Row>
        {installedModels.length === 0 && (
          <Alert
            style={{ marginTop: 12 }}
            type="warning"
            showIcon
            message="Chưa có model TTS nào sẵn sàng."
          />
        )}
      </Card>

      <Card
        size="small"
        title={
          <Space>
            <AudioOutlined /> Thử nghiệm giọng nói
          </Space>
        }
      >
        <Space.Compact style={{ width: '100%' }}>
          <Input
            value={testText}
            onChange={(e) => setTestText(e.target.value)}
            placeholder="Nhập văn bản cần đọc..."
            onPressEnter={synthesizeTest}
            disabled={synthesizing}
          />
          <Button 
            type="primary" 
            icon={<PlayCircleOutlined />} 
            onClick={synthesizeTest}
            loading={synthesizing}
            disabled={!testText.trim() || installedModels.length === 0}
          >
            Nghe thử
          </Button>
        </Space.Compact>
      </Card>
    </div>
  );
};
