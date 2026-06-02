import React, { useEffect, useState } from 'react';
import {
  Drawer, Form, Input, Button, Radio, TimePicker, InputNumber, Collapse,
  Tag, Typography, theme, Spin, Empty, Tooltip, Space, message, Popconfirm,
} from 'antd';
import {
  MessageOutlined, PauseCircleOutlined, PlayCircleOutlined,
  DeleteOutlined, SaveOutlined,
} from '@ant-design/icons';
import dayjs, { Dayjs } from 'dayjs';
import { useNavigate } from 'react-router-dom';
import type {
  UseSpaceHook, SpaceScheduleDetail, ScheduleUpdatePayload,
} from '../../../hooks/useSpace';

const { Text, Title } = Typography;
const { TextArea } = Input;

const WEEKDAY_OPTS = [
  { label: 'CN', value: 0 }, { label: 'T2', value: 1 }, { label: 'T3', value: 2 },
  { label: 'T4', value: 3 }, { label: 'T5', value: 4 }, { label: 'T6', value: 5 },
  { label: 'T7', value: 6 },
];

type Freq = 'daily' | 'weekdays' | 'weekly' | 'monthly' | 'custom';

interface ParsedCron {
  freq: Freq;
  time: Dayjs;
  weekday?: number;
  dayOfMonth?: number;
  raw: string;
}

function parseCron(expr: string): ParsedCron {
  const parts = (expr || '').trim().split(/\s+/);
  const raw = expr;
  if (parts.length !== 5) return { freq: 'custom', time: dayjs(), raw };
  const [m, h, dom, , dow] = parts;
  const mm = Number(m), hh = Number(h);
  const time = dayjs().hour(isFinite(hh) ? hh : 0).minute(isFinite(mm) ? mm : 0);
  if (dom === '*' && dow === '*') return { freq: 'daily', time, raw };
  if (dom === '*' && dow === '1-5') return { freq: 'weekdays', time, raw };
  if (dom === '*' && /^\d$/.test(dow)) return { freq: 'weekly', time, weekday: Number(dow), raw };
  if (/^\d+$/.test(dom) && dow === '*') return { freq: 'monthly', time, dayOfMonth: Number(dom), raw };
  return { freq: 'custom', time, raw };
}

interface Props {
  scheduleId: string;
  hook: UseSpaceHook;
  onClose: () => void;
}

export function ScheduleDetailPanel({ scheduleId, hook, onClose }: Props) {
  const { token } = theme.useToken();
  const [form] = Form.useForm();
  const navigate = useNavigate();
  const [detail, setDetail] = useState<SpaceScheduleDetail | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [freq, setFreq] = useState<Freq>('daily');
  const [useAdvanced, setUseAdvanced] = useState(false);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    hook.getScheduleDetail(scheduleId).then(d => {
      if (cancelled || !d) return;
      setDetail(d);
      const parsed = parseCron(d.schedule_value);
      setFreq(parsed.freq);
      setUseAdvanced(parsed.freq === 'custom');
      form.setFieldsValue({
        label: d.label,
        prompt: d.prompt,
        time: parsed.time,
        weekday: parsed.weekday ?? 1,
        day_of_month: parsed.dayOfMonth ?? 1,
        cron_advanced: parsed.raw,
      });
      setLoading(false);
    });
    return () => { cancelled = true; };
  }, [scheduleId]);

  const handleSave = async () => {
    try {
      const vals = await form.validateFields();
      setSaving(true);
      const payload: ScheduleUpdatePayload = useAdvanced
        ? {
            prompt: vals.prompt,
            label: vals.label,
            cron_advanced: vals.cron_advanced,
          }
        : {
            prompt: vals.prompt,
            label: vals.label,
            time_local: (vals.time as Dayjs).format('HH:mm'),
            frequency: freq === 'custom' ? 'daily' : freq,
            weekday: freq === 'weekly' ? vals.weekday : undefined,
            day_of_month: freq === 'monthly' ? vals.day_of_month : undefined,
          };
      const res = await hook.updateSchedule(scheduleId, payload);
      if (res) {
        message.success('Đã lưu');
        setDetail(prev => prev ? { ...prev, ...res } : prev);
      }
    } catch {
      /* validation */
    } finally {
      setSaving(false);
    }
  };

  const togglePause = async () => {
    if (!detail) return;
    const next = detail.status === 'active' ? 'paused' : 'active';
    const res = await hook.updateSchedule(scheduleId, { status: next });
    if (res) setDetail(prev => prev ? { ...prev, ...res } : prev);
  };

  const handleDelete = async () => {
    await hook.cancelSchedule(scheduleId);
    onClose();
  };

  const openChat = () => {
    if (!detail) return;
    navigate(`/chats?jid=${encodeURIComponent(detail.chat_jid)}`);
  };

  const formatTs = (v: string | null) => {
    if (!v) return '—';
    return new Date(v).toLocaleString('vi');
  };

  return (
    <Drawer
      open
      onClose={onClose}
      width={Math.min(560, window.innerWidth - 80)}
      title={detail?.label ?? 'Chi tiết lịch'}
      destroyOnClose
      extra={
        detail && (
          <Space>
            <Tooltip title="Mở chat session">
              <Button icon={<MessageOutlined />} onClick={openChat}>Chat</Button>
            </Tooltip>
            <Tooltip title={detail.status === 'active' ? 'Tạm dừng' : 'Kích hoạt'}>
              <Button
                icon={detail.status === 'active' ? <PauseCircleOutlined /> : <PlayCircleOutlined />}
                onClick={togglePause}
              />
            </Tooltip>
            <Popconfirm
              title="Xoá lịch và chat session?"
              onConfirm={handleDelete}
              okText="Xoá"
              cancelText="Huỷ"
            >
              <Button danger icon={<DeleteOutlined />} />
            </Popconfirm>
          </Space>
        )
      }
    >
      {loading && <div className="flex justify-center py-8"><Spin /></div>}
      {!loading && !detail && <Empty description="Không tìm thấy lịch" />}
      {!loading && detail && (
        <>
          <div className="mb-3">
            <Tag color={detail.status === 'active' ? 'green' : 'orange'}>
              {detail.status === 'active' ? 'Đang chạy' : detail.status === 'paused' ? 'Tạm dừng' : 'Đã huỷ'}
            </Tag>
            <Text type="secondary" className="text-xs ml-2">
              Lần tới: {formatTs(detail.next_run)} · Lần cuối: {formatTs(detail.last_run)}
            </Text>
          </div>

          <Form form={form} layout="vertical">
            <Form.Item name="label" label="Tên gọi">
              <Input placeholder="Tên hiển thị" />
            </Form.Item>

            <Form.Item
              name="prompt"
              label="Yêu cầu cho agent"
              rules={[{ required: true, message: 'Nhập nội dung' }]}
            >
              <TextArea rows={3} />
            </Form.Item>

            {!useAdvanced && (
              <>
                <Form.Item label="Tần suất">
                  <Radio.Group
                    optionType="button"
                    buttonStyle="solid"
                    value={freq === 'custom' ? 'daily' : freq}
                    onChange={e => setFreq(e.target.value)}
                    options={[
                      { label: 'Mỗi ngày', value: 'daily' },
                      { label: 'Th 2–6', value: 'weekdays' },
                      { label: 'Hàng tuần', value: 'weekly' },
                      { label: 'Hàng tháng', value: 'monthly' },
                    ]}
                  />
                </Form.Item>

                <Form.Item name="time" label="Giờ chạy (giờ máy)">
                  <TimePicker format="HH:mm" minuteStep={5} style={{ width: 140 }} />
                </Form.Item>

                {freq === 'weekly' && (
                  <Form.Item name="weekday" label="Thứ trong tuần">
                    <Radio.Group optionType="button" options={WEEKDAY_OPTS} />
                  </Form.Item>
                )}

                {freq === 'monthly' && (
                  <Form.Item name="day_of_month" label="Ngày trong tháng">
                    <InputNumber min={1} max={28} />
                  </Form.Item>
                )}
              </>
            )}

            <Collapse
              ghost
              activeKey={useAdvanced ? ['adv'] : []}
              onChange={k => setUseAdvanced((k as string[]).includes('adv'))}
              items={[{
                key: 'adv',
                label: 'Nâng cao: Cron expression',
                children: (
                  <Form.Item name="cron_advanced">
                    <Input placeholder="0 7 * * *" className="font-mono" />
                  </Form.Item>
                ),
              }]}
            />

            <Button
              type="primary"
              icon={<SaveOutlined />}
              loading={saving}
              onClick={handleSave}
            >
              Lưu thay đổi
            </Button>
          </Form>

          <Title level={5} className="mt-6">Lịch sử chạy ({detail.runs.length})</Title>
          {detail.runs.length === 0 && (
            <Empty description="Chưa có lần chạy nào" className="py-4" />
          )}
          <div className="space-y-2">
            {detail.runs.map(r => (
              <div
                key={r.id}
                className="p-2 rounded border text-xs"
                style={{
                  borderColor: token.colorBorderSecondary,
                  background: token.colorFillQuaternary,
                }}
              >
                <div className="flex items-center justify-between gap-2 mb-1">
                  <Tag color={r.status === 'success' ? 'green' : 'red'} className="!text-xs">
                    {r.status === 'success' ? 'OK' : 'Lỗi'}
                  </Tag>
                  <Text type="secondary" style={{ fontSize: 11 }}>
                    {new Date(r.run_at).toLocaleString('vi')}
                    {r.duration_ms ? ` · ${Math.round(r.duration_ms / 1000)}s` : ''}
                  </Text>
                </div>
                {r.result && (
                  <pre className="whitespace-pre-wrap break-words m-0 text-xs">{r.result}</pre>
                )}
                {r.error && (
                  <pre
                    className="whitespace-pre-wrap break-words m-0 text-xs"
                    style={{ color: token.colorError }}
                  >
                    {r.error}
                  </pre>
                )}
              </div>
            ))}
          </div>
        </>
      )}
    </Drawer>
  );
}
