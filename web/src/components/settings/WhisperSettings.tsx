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
  Input,
} from 'antd';
import {
  CloudDownloadOutlined,
  DeleteOutlined,
  StopOutlined,
  ReloadOutlined,
  CheckCircleOutlined,
  AudioOutlined,
  AudioMutedOutlined,
  InboxOutlined,
  CopyOutlined,
  FileTextOutlined,
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

interface WhisperModel {
  id: string;
  label: string;
  approx_size_gb: number;
  default_language: string;
  installed: boolean;
  on_disk_path: string;
  download: DownloadState | null;
}

// Languages Whisper handles well; Vietnamese first (project priority).
const LANGUAGES: { code: string; label: string }[] = [
  { code: 'vi', label: 'Tiếng Việt' },
  { code: 'en', label: 'English' },
  { code: 'zh', label: '中文' },
  { code: 'ja', label: '日本語' },
  { code: 'ko', label: '한국어' },
  { code: 'fr', label: 'Français' },
  { code: 'de', label: 'Deutsch' },
  { code: 'es', label: 'Español' },
  { code: 'ru', label: 'Русский' },
  { code: 'th', label: 'ไทย' },
];

function fmtBytes(n: number): string {
  if (!n) return '0 B';
  const u = ['B', 'KB', 'MB', 'GB'];
  const i = Math.min(Math.floor(Math.log(n) / Math.log(1024)), u.length - 1);
  return `${(n / 1024 ** i).toFixed(1)} ${u[i]}`;
}

/** Encode mono Float32 PCM as a 16-bit PCM WAV blob (decodable by the backend). */
function encodeWav(samples: Float32Array, sampleRate: number): Blob {
  const buffer = new ArrayBuffer(44 + samples.length * 2);
  const view = new DataView(buffer);
  const writeStr = (off: number, s: string) => {
    for (let i = 0; i < s.length; i++) view.setUint8(off + i, s.charCodeAt(i));
  };
  writeStr(0, 'RIFF');
  view.setUint32(4, 36 + samples.length * 2, true);
  writeStr(8, 'WAVE');
  writeStr(12, 'fmt ');
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true); // PCM
  view.setUint16(22, 1, true); // mono
  view.setUint32(24, sampleRate, true);
  view.setUint32(28, sampleRate * 2, true);
  view.setUint16(32, 2, true);
  view.setUint16(34, 16, true);
  writeStr(36, 'data');
  view.setUint32(40, samples.length * 2, true);
  let off = 44;
  for (let i = 0; i < samples.length; i++) {
    const s = Math.max(-1, Math.min(1, samples[i]));
    view.setInt16(off, s < 0 ? s * 0x8000 : s * 0x7fff, true);
    off += 2;
  }
  return new Blob([view], { type: 'audio/wav' });
}

export const WhisperSettings: React.FC = () => {
  const [models, setModels] = useState<WhisperModel[]>([]);
  const [loading, setLoading] = useState(true);
  const [hfInput, setHfInput] = useState('');
  const pollRef = useRef<number | null>(null);

  const [activeModel, setActiveModel] = useState<string | undefined>();
  const [language, setLanguage] = useState('vi');

  const refresh = useCallback(async () => {
    try {
      const [listRes, setRes] = await Promise.all([
        fetch('/api/whisper/models'),
        fetch('/api/whisper/settings'),
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
      message.error(`Không tải được danh sách Whisper: ${e.message}`);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  // Poll while a download is active.
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
    const res = await fetch(`/api/whisper/models/${encodeURIComponent(id)}/download`, {
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
    await fetch(`/api/whisper/models/${encodeURIComponent(id)}/cancel`, { method: 'POST' });
    refresh();
  };

  const remove = async (id: string) => {
    await fetch(`/api/whisper/models/${encodeURIComponent(id)}`, { method: 'DELETE' });
    message.success('Đã xoá model');
    refresh();
  };

  const saveSelection = async (model_id?: string, lang?: string) => {
    const body = { model_id: model_id ?? activeModel, language: lang ?? language };
    const res = await fetch('/api/whisper/settings', {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    });
    if (res.ok) message.success('Đã lưu lựa chọn');
    else message.error(`Lưu thất bại: ${await res.text()}`);
  };

  const installedModels = models.filter((m) => m.installed);

  const columns = [
    {
      title: 'Model',
      dataIndex: 'label',
      render: (label: string, m: WhisperModel) => (
        <div>
          <div style={{ fontWeight: 600 }}>{label}</div>
          <Text type="secondary" style={{ fontSize: 12 }}>
            {m.id} · ~{m.approx_size_gb} GB
          </Text>
        </div>
      ),
    },
    {
      title: 'Trạng thái',
      key: 'status',
      width: 280,
      render: (_: any, m: WhisperModel) => {
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
      render: (_: any, m: WhisperModel) => {
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
          Whisper ASR
        </Title>
        <Button icon={<ReloadOutlined />} onClick={refresh}>
          Làm mới
        </Button>
      </Space>
      <Paragraph type="secondary">
        Tải model Whisper (chạy native trên MLX, Apple Silicon), chọn model + ngôn ngữ, rồi ghi âm
        hoặc tải file audio lên để nhận diện giọng nói. Ưu tiên tiếng Việt.
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

      <Card size="small" title="Cài từ HuggingFace (tuỳ chọn)" style={{ marginBottom: 16 }}>
        <Space.Compact style={{ width: '100%' }}>
          <Input
            placeholder="org/repo (vd. mlx-community/whisper-large-v3-turbo)"
            value={hfInput}
            onChange={(e) => setHfInput(e.target.value)}
            allowClear
          />
          <Button
            type="primary"
            icon={<CloudDownloadOutlined />}
            disabled={!hfInput.trim()}
            onClick={() => download(hfInput.trim())}
          >
            Tải
          </Button>
        </Space.Compact>
        <Text type="secondary" style={{ fontSize: 12 }}>
          Repo phải chứa tokenizer.json hoặc nằm trong danh mục có sẵn (mlx-community/whisper-large-v3-turbo).
        </Text>
      </Card>

      <Card size="small" title="Lựa chọn nhận diện" style={{ marginBottom: 16 }}>
        <Space wrap>
          <span>
            <Text type="secondary" style={{ marginRight: 8 }}>
              Model:
            </Text>
            <Select
              style={{ minWidth: 320 }}
              placeholder="Chọn model đã cài"
              value={activeModel}
              onChange={(v) => {
                setActiveModel(v);
                saveSelection(v, undefined);
              }}
              options={installedModels.map((m) => ({ value: m.id, label: m.label }))}
            />
          </span>
          <span>
            <Text type="secondary" style={{ marginRight: 8 }}>
              Ngôn ngữ:
            </Text>
            <Select
              style={{ minWidth: 140 }}
              value={language}
              onChange={(v) => {
                setLanguage(v);
                saveSelection(undefined, v);
              }}
              options={LANGUAGES.map((l) => ({ value: l.code, label: l.label }))}
            />
          </span>
        </Space>
        {installedModels.length === 0 && (
          <Alert
            style={{ marginTop: 12 }}
            type="info"
            showIcon
            message="Chưa có model nào được cài. Hãy tải model ở bảng trên."
          />
        )}
      </Card>

      <AudioTranscribe language={language} disabled={installedModels.length === 0} />
    </div>
  );
};

// ── Recording + upload + transcription panel ─────────────────────────────────

const AudioTranscribe: React.FC<{ language: string; disabled: boolean }> = ({
  language,
  disabled,
}) => {
  const [recording, setRecording] = useState(false);
  const [elapsed, setElapsed] = useState(0);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState('');

  const ctxRef = useRef<AudioContext | null>(null);
  const procRef = useRef<ScriptProcessorNode | null>(null);
  const srcRef = useRef<MediaStreamAudioSourceNode | null>(null);
  const streamRef = useRef<MediaStream | null>(null);
  const chunksRef = useRef<Float32Array[]>([]);
  const timerRef = useRef<number | null>(null);

  const cleanup = () => {
    procRef.current?.disconnect();
    srcRef.current?.disconnect();
    streamRef.current?.getTracks().forEach((t) => t.stop());
    ctxRef.current?.close().catch(() => {});
    if (timerRef.current) window.clearInterval(timerRef.current);
    procRef.current = null;
    srcRef.current = null;
    streamRef.current = null;
    ctxRef.current = null;
    timerRef.current = null;
  };

  useEffect(() => cleanup, []);

  const startRecording = async () => {
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      const ctx = new AudioContext();
      const source = ctx.createMediaStreamSource(stream);
      const processor = ctx.createScriptProcessor(4096, 1, 1);
      chunksRef.current = [];
      processor.onaudioprocess = (e) => {
        chunksRef.current.push(new Float32Array(e.inputBuffer.getChannelData(0)));
      };
      source.connect(processor);
      processor.connect(ctx.destination);
      ctxRef.current = ctx;
      srcRef.current = source;
      procRef.current = processor;
      streamRef.current = stream;
      setElapsed(0);
      timerRef.current = window.setInterval(() => setElapsed((s) => s + 1), 1000);
      setRecording(true);
    } catch (e: any) {
      message.error(`Không truy cập được micro: ${e.message}`);
    }
  };

  const stopAndTranscribe = async () => {
    const sampleRate = ctxRef.current?.sampleRate ?? 48000;
    const chunks = chunksRef.current;
    cleanup();
    setRecording(false);

    const total = chunks.reduce((n, c) => n + c.length, 0);
    if (total === 0) {
      message.warning('Bản ghi rỗng');
      return;
    }
    const merged = new Float32Array(total);
    let off = 0;
    for (const c of chunks) {
      merged.set(c, off);
      off += c.length;
    }
    const wav = encodeWav(merged, sampleRate);
    await transcribe(wav, 'recording.wav');
  };

  const transcribe = async (blob: Blob, filename: string) => {
    setBusy(true);
    setResult('');
    try {
      const fd = new FormData();
      fd.append('audio', blob, filename);
      fd.append('language', language);
      const res = await fetch('/api/whisper/transcribe', { method: 'POST', body: fd });
      const txt = await res.text();
      if (!res.ok) {
        message.error(`Nhận diện thất bại: ${txt}`);
        return;
      }
      const j = JSON.parse(txt);
      setResult(j.text || '');
      if (!j.text) message.info('Không nhận được văn bản');
    } catch (e: any) {
      message.error(`Lỗi: ${e.message}`);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Card
      size="small"
      title={
        <Space>
          <AudioOutlined /> Ghi âm / Tải file audio
        </Space>
      }
    >
      <Space direction="vertical" style={{ width: '100%' }} size="middle">
        <Space>
          {!recording ? (
            <Button
              type="primary"
              icon={<AudioOutlined />}
              disabled={disabled || busy}
              onClick={startRecording}
            >
              Bắt đầu ghi âm
            </Button>
          ) : (
            <Button danger icon={<AudioMutedOutlined />} onClick={stopAndTranscribe}>
              Dừng &amp; nhận diện ({elapsed}s)
            </Button>
          )}
          {recording && <Tag color="processing">Đang ghi… {elapsed}s</Tag>}
        </Space>

        <Upload.Dragger
          accept="audio/*,.wav,.mp3,.m4a,.flac,.ogg"
          multiple={false}
          showUploadList={false}
          disabled={disabled || busy || recording}
          beforeUpload={(file) => {
            transcribe(file, file.name);
            return false;
          }}
        >
          <p className="ant-upload-drag-icon">
            <InboxOutlined />
          </p>
          <p className="ant-upload-text">Kéo thả hoặc bấm để chọn file audio</p>
          <p className="ant-upload-hint">Hỗ trợ wav, mp3, m4a, flac, ogg</p>
        </Upload.Dragger>

        {busy && (
          <Space>
            <Spin /> <Text type="secondary">Đang nhận diện…</Text>
          </Space>
        )}

        {result && (
          <Card
            size="small"
            type="inner"
            title={
              <Space>
                <FileTextOutlined /> Kết quả
              </Space>
            }
            extra={
              <Button
                size="small"
                icon={<CopyOutlined />}
                onClick={() => {
                  navigator.clipboard.writeText(result);
                  message.success('Đã copy');
                }}
              >
                Copy
              </Button>
            }
          >
            <Paragraph style={{ margin: 0, whiteSpace: 'pre-wrap' }}>{result}</Paragraph>
          </Card>
        )}
      </Space>
    </Card>
  );
};
