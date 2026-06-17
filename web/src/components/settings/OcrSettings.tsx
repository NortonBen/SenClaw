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
  Upload,
  Spin,
} from 'antd';
import {
  CloudDownloadOutlined,
  DeleteOutlined,
  StopOutlined,
  ReloadOutlined,
  CheckCircleOutlined,
  ScanOutlined,
  InboxOutlined,
  StarFilled,
  LinkOutlined,
} from '@ant-design/icons';
import { Input as AntInput, Form } from 'antd';

const { Title, Text, Paragraph } = Typography;

type DownloadStatus = 'queued' | 'downloading' | 'done' | 'error' | 'cancelled';

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

interface OcrModel {
  id: string;
  label: string;
  description?: string;
  approx_size_mb: number;
  default_language: string;
  version?: number;
  is_default?: boolean;
  installed: boolean;
  on_disk_path: string;
  download: DownloadState | null;
}

interface OcrBlock {
  text: string;
  confidence: number;
  bbox: [number, number, number, number];
}

const LANGUAGES: { code: string; label: string }[] = [
  { code: 'vi', label: 'Tiếng Việt' },
  { code: 'en', label: 'English' },
  { code: 'zh', label: '中文' },
  { code: 'ja', label: '日本語' },
  { code: 'ko', label: '한국어' },
  { code: 'fr', label: 'Français' },
  { code: 'de', label: 'Deutsch' },
  { code: 'es', label: 'Español' },
  { code: 'multi', label: 'Auto / Mixed' },
];

function fmtBytes(n: number): string {
  if (!n) return '0 B';
  const u = ['B', 'KB', 'MB', 'GB'];
  const i = Math.min(Math.floor(Math.log(n) / Math.log(1024)), u.length - 1);
  return `${(n / 1024 ** i).toFixed(1)} ${u[i]}`;
}

export const OcrSettings: React.FC = () => {
  const [models, setModels] = useState<OcrModel[]>([]);
  const [loading, setLoading] = useState(true);
  const pollRef = useRef<number | null>(null);

  const [activeModel, setActiveModel] = useState<string | undefined>();
  const [language, setLanguage] = useState('vi');

  const [recognizing, setRecognizing] = useState(false);
  const [resultText, setResultText] = useState<string>('');
  const [resultBlocks, setResultBlocks] = useState<OcrBlock[]>([]);

  const refresh = useCallback(async () => {
    try {
      const [listRes, setRes] = await Promise.all([
        fetch('/api/ocr/models'),
        fetch('/api/ocr/settings'),
      ]);
      if (!listRes.ok) throw new Error(`list failed (${listRes.status})`);
      const list = await listRes.json();
      setModels(list.models || []);
      if (setRes.ok) {
        const s = await setRes.json();
        if (s.model_id) setActiveModel(s.model_id);
        if (s.language) setLanguage(s.language);
      }
    } catch (e: any) {
      message.error(`Không tải được danh sách OCR: ${e.message}`);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  useEffect(() => {
    const active = models.some(
      (m) => m.download && ['queued', 'downloading'].includes(m.download.status)
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
    const res = await fetch(`/api/ocr/models/${encodeURIComponent(id)}/download`, {
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
    await fetch(`/api/ocr/models/${encodeURIComponent(id)}/cancel`, { method: 'POST' });
    refresh();
  };

  const remove = async (id: string) => {
    await fetch(`/api/ocr/models/${encodeURIComponent(id)}`, { method: 'DELETE' });
    message.success('Đã xoá model');
    refresh();
  };

  const saveSelection = async (model_id?: string, lang?: string) => {
    const body = { model_id: model_id ?? activeModel, language: lang ?? language };
    const res = await fetch('/api/ocr/settings', {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    });
    if (res.ok) message.success('Đã lưu lựa chọn');
    else message.error(`Lưu thất bại: ${await res.text()}`);
  };

  const customDownload = async (values: {
    id: string;
    det_url: string;
    rec_url: string;
    keys_url: string;
  }) => {
    const res = await fetch('/api/ocr/models/custom', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(values),
    });
    if (!res.ok) {
      message.error(`Tải custom thất bại: ${await res.text()}`);
      return;
    }
    message.success(`Bắt đầu tải ${values.id}`);
    refresh();
  };

  const recognizeFile = async (file: File) => {
    setRecognizing(true);
    setResultText('');
    setResultBlocks([]);
    try {
      const form = new FormData();
      form.append('image', file);
      form.append('language', language);
      const res = await fetch('/api/ocr/recognize', { method: 'POST', body: form });
      if (!res.ok) {
        const errText = await res.text();
        message.error(`OCR thất bại: ${errText}`);
        return;
      }
      const data = await res.json();
      setResultText(data.text || '');
      setResultBlocks(data.blocks || []);
      message.success(`Đã trích xuất ${data.blocks?.length ?? 0} block`);
    } catch (e: any) {
      message.error(`OCR lỗi: ${e.message}`);
    } finally {
      setRecognizing(false);
    }
  };

  const installedModels = models.filter((m) => m.installed);

  const columns = [
    {
      title: 'Model',
      dataIndex: 'label',
      render: (label: string, m: OcrModel) => (
        <div>
          <Space size={6} wrap>
            <Text strong>{label}</Text>
            {m.is_default && (
              <Tag color="gold" icon={<StarFilled />} style={{ margin: 0 }}>
                Mặc định
              </Tag>
            )}
            {m.version ? <Tag color="blue">v{m.version}</Tag> : null}
          </Space>
          <br />
          {m.description && (
            <Text type="secondary" style={{ fontSize: 12 }}>
              {m.description}
            </Text>
          )}
          {m.description && <br />}
          <Text type="secondary" style={{ fontSize: 11, fontFamily: 'monospace' }}>
            {m.id}
          </Text>
        </div>
      ),
    },
    {
      title: 'Kích thước',
      dataIndex: 'approx_size_mb',
      width: 110,
      render: (mb: number) => <Text>{mb ? `~${mb.toFixed(0)} MB` : '—'}</Text>,
    },
    {
      title: 'Ngôn ngữ',
      dataIndex: 'default_language',
      width: 90,
      render: (l: string) => <Tag>{l}</Tag>,
    },
    {
      title: 'Trạng thái',
      width: 280,
      render: (_: any, m: OcrModel) => {
        if (m.download && ['queued', 'downloading'].includes(m.download.status)) {
          const pct = m.download.total_bytes
            ? Math.round((m.download.downloaded_bytes / m.download.total_bytes) * 100)
            : Math.round((m.download.files_done / Math.max(1, m.download.files_total)) * 100);
          return (
            <div>
              <Progress percent={pct} size="small" />
              <Text type="secondary" style={{ fontSize: 12 }}>
                {m.download.current_file ?? '...'} ·{' '}
                {fmtBytes(m.download.downloaded_bytes)}
              </Text>
            </div>
          );
        }
        if (m.installed) {
          return (
            <Tag color="green" icon={<CheckCircleOutlined />}>
              Đã cài
            </Tag>
          );
        }
        if (m.download?.status === 'error') {
          return <Tag color="red">Lỗi: {m.download.error}</Tag>;
        }
        if (m.download?.status === 'cancelled') {
          return <Tag color="orange">Đã huỷ</Tag>;
        }
        return <Tag>Chưa cài</Tag>;
      },
    },
    {
      title: '',
      width: 200,
      render: (_: any, m: OcrModel) => (
        <Space>
          {!m.installed &&
            !(m.download && ['queued', 'downloading'].includes(m.download.status)) && (
              <Button
                size="small"
                type="primary"
                icon={<CloudDownloadOutlined />}
                onClick={() => download(m.id)}
              >
                Tải
              </Button>
            )}
          {m.download && ['queued', 'downloading'].includes(m.download.status) && (
            <Button size="small" icon={<StopOutlined />} danger onClick={() => cancel(m.id)}>
              Huỷ
            </Button>
          )}
          {m.installed && (
            <Popconfirm title={`Xoá model ${m.id}?`} onConfirm={() => remove(m.id)}>
              <Button size="small" icon={<DeleteOutlined />} danger />
            </Popconfirm>
          )}
        </Space>
      ),
    },
  ];

  return (
    <div>
      <Title level={3} style={{ marginTop: 0 }}>
        OCR (PaddleOCR + MNN)
      </Title>
      <Paragraph type="secondary">
        Trích xuất chữ từ ảnh & tài liệu hoàn toàn trên thiết bị. macOS dùng Metal/CoreML
        khi build với <code>--features ocr-paddle-metal</code>; Linux/Windows dùng CPU
        (<code>--features ocr-paddle</code>).
      </Paragraph>

      <Card title="Catalog model" extra={
        <Button icon={<ReloadOutlined />} onClick={refresh} loading={loading}>
          Làm mới
        </Button>
      } style={{ marginBottom: 24 }}>
        <Table
          rowKey="id"
          dataSource={models}
          columns={columns as any}
          loading={loading}
          pagination={false}
          size="small"
        />
      </Card>

      <Card
        title={
          <Space>
            <LinkOutlined /> Tải model từ URL tuỳ chỉnh
          </Space>
        }
        style={{ marginBottom: 24 }}
      >
        <Paragraph type="secondary" style={{ marginTop: 0 }}>
          Nếu mirror trong catalog không hoạt động, dán URL trực tiếp tới 3 file
          <code> det.mnn</code>, <code>rec.mnn</code> và <code>keys.txt</code>.
        </Paragraph>
        <Form
          layout="vertical"
          onFinish={customDownload}
          initialValues={{
            id: 'my-custom-ocr',
            det_url: '',
            rec_url: '',
            keys_url: '',
          }}
        >
          <Form.Item
            label="Tên định danh (id)"
            name="id"
            rules={[{ required: true, pattern: /^[A-Za-z0-9._-]+$/, message: 'Chỉ chữ/số/. _ -' }]}
          >
            <AntInput placeholder="my-custom-ocr" />
          </Form.Item>
          <Form.Item
            label="URL detection (.mnn)"
            name="det_url"
            rules={[{ required: true, type: 'url', message: 'URL không hợp lệ' }]}
          >
            <AntInput placeholder="https://.../det.mnn" />
          </Form.Item>
          <Form.Item
            label="URL recognition (.mnn)"
            name="rec_url"
            rules={[{ required: true, type: 'url', message: 'URL không hợp lệ' }]}
          >
            <AntInput placeholder="https://.../rec.mnn" />
          </Form.Item>
          <Form.Item
            label="URL keys (.txt)"
            name="keys_url"
            rules={[{ required: true, type: 'url', message: 'URL không hợp lệ' }]}
          >
            <AntInput placeholder="https://.../keys.txt" />
          </Form.Item>
          <Button type="primary" htmlType="submit" icon={<CloudDownloadOutlined />}>
            Tải model tuỳ chỉnh
          </Button>
        </Form>
      </Card>

      <Card title="Chọn model & ngôn ngữ" style={{ marginBottom: 24 }}>
        {installedModels.length === 0 ? (
          <Alert
            type="info"
            message="Chưa có model OCR nào được cài đặt."
            description="Tải một model từ bảng phía trên (gợi ý: PP-OCRv5_mobile_latin cho tiếng Việt + Anh)."
          />
        ) : (
          <Space direction="vertical" style={{ width: '100%' }}>
            <div>
              <Text strong>Model đang dùng:</Text>
              <Select
                style={{ width: '100%', marginTop: 6 }}
                value={activeModel}
                placeholder="Chọn model"
                options={installedModels.map((m) => ({ value: m.id, label: m.label }))}
                onChange={(v) => {
                  setActiveModel(v);
                  saveSelection(v, language);
                }}
              />
            </div>
            <div>
              <Text strong>Ngôn ngữ mặc định:</Text>
              <Select
                style={{ width: '100%', marginTop: 6 }}
                value={language}
                options={LANGUAGES.map((l) => ({ value: l.code, label: l.label }))}
                onChange={(v) => {
                  setLanguage(v);
                  saveSelection(activeModel, v);
                }}
              />
            </div>
          </Space>
        )}
      </Card>

      <Card title="Thử nghiệm: kéo & thả ảnh">
        <Upload.Dragger
          accept="image/png,image/jpeg,image/webp,image/bmp,image/gif"
          showUploadList={false}
          beforeUpload={(file) => {
            recognizeFile(file as File);
            return false;
          }}
          disabled={recognizing || installedModels.length === 0}
        >
          <p className="ant-upload-drag-icon">
            {recognizing ? <Spin /> : <InboxOutlined />}
          </p>
          <p className="ant-upload-text">
            {recognizing ? 'Đang nhận diện...' : 'Kéo ảnh vào đây hoặc bấm để chọn'}
          </p>
          <p className="ant-upload-hint">PNG, JPG, WebP, BMP, GIF</p>
        </Upload.Dragger>

        {(resultText || resultBlocks.length > 0) && (
          <div style={{ marginTop: 16 }}>
            <Title level={5}>
              <ScanOutlined /> Kết quả ({resultBlocks.length} block)
            </Title>
            <pre
              style={{
                background: '#fafafa',
                padding: 12,
                borderRadius: 8,
                whiteSpace: 'pre-wrap',
                maxHeight: 360,
                overflow: 'auto',
              }}
            >
              {resultText || '(không có chữ)'}
            </pre>
            {resultBlocks.some((b) => b.confidence < 0.6) && (
              <Alert
                style={{ marginTop: 8 }}
                type="warning"
                showIcon
                message="Một số block có độ tin cậy thấp (<60%) — kết quả có thể sai."
              />
            )}
          </div>
        )}
      </Card>
    </div>
  );
};
