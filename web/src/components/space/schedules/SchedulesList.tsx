import React, { useEffect, useMemo, useState } from 'react';
import {
  Button, Modal, Form, Input, Tag, Empty, Spin, Popconfirm,
  Typography, theme, Tooltip, Radio, TimePicker, Collapse, InputNumber, Alert,
} from 'antd';
import {
  PlusOutlined, DeleteOutlined, ClockCircleOutlined, MessageOutlined,
  ReloadOutlined, CheckCircleTwoTone, CloseCircleTwoTone, RobotOutlined,
} from '@ant-design/icons';
import dayjs, { Dayjs } from 'dayjs';
import { useNavigate } from 'react-router-dom';
import type { UseSpaceHook, ScheduleCreatePayload, SpaceSchedule, AgentModeType } from '../../../hooks/useSpace';
import { ScheduleDetailPanel } from './ScheduleDetailPanel';

const { Text, Title } = Typography;
const { TextArea } = Input;

const STATUS_MAP: Record<string, { color: string; label: string }> = {
  active: { color: 'green', label: 'Đang chạy' },
  paused: { color: 'orange', label: 'Tạm dừng' },
  completed: { color: 'default', label: 'Đã hủy' },
};

const WEEKDAY_OPTS = [
  { label: 'CN', value: 0 },
  { label: 'T2', value: 1 },
  { label: 'T3', value: 2 },
  { label: 'T4', value: 3 },
  { label: 'T5', value: 4 },
  { label: 'T6', value: 5 },
  { label: 'T7', value: 6 },
];

interface Props {
  hook: UseSpaceHook;
}

function describeCron(s: SpaceSchedule): string {
  const cron = (s.schedule_value || '').split(/\s+/);
  if (cron.length !== 5) return s.schedule_value;
  const [m, h, dom, , dow] = cron;
  const hhmm = `${h.padStart(2, '0')}:${m.padStart(2, '0')}`;
  if (dom === '*' && dow === '*') return `Mỗi ngày · ${hhmm}`;
  if (dom === '*' && dow === '1-5') return `Th 2–6 · ${hhmm}`;
  if (dom === '*' && /^\d$/.test(dow)) {
    const label = WEEKDAY_OPTS.find(w => w.value === Number(dow))?.label ?? dow;
    return `Mỗi ${label} · ${hhmm}`;
  }
  if (/^\d+$/.test(dom) && dow === '*') return `Ngày ${dom} mỗi tháng · ${hhmm}`;
  return s.schedule_value;
}

export function SchedulesList({ hook }: Props) {
  const { token } = theme.useToken();
  const navigate = useNavigate();
  const [showAdd, setShowAdd] = useState(false);
  const [openDetailId, setOpenDetailId] = useState<string | null>(null);

  useEffect(() => {
    hook.loadSchedules();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Sort by next_run ascending so the soonest-up-next is first.
  const byNextRun = (a: SpaceSchedule, b: SpaceSchedule) => {
    const va = a.next_run ? new Date(a.next_run).getTime() : Number.MAX_SAFE_INTEGER;
    const vb = b.next_run ? new Date(b.next_run).getTime() : Number.MAX_SAFE_INTEGER;
    return va - vb;
  };
  const active = useMemo(
    () => hook.schedules.filter(s => s.status === 'active').sort(byNextRun),
    [hook.schedules],
  );
  const paused = useMemo(
    () => hook.schedules.filter(s => s.status === 'paused').sort(byNextRun),
    [hook.schedules],
  );
  const completed = useMemo(
    () => hook.schedules.filter(s => s.status === 'completed'),
    [hook.schedules],
  );

  const openChat = (jid: string) => navigate(`/chats?jid=${encodeURIComponent(jid)}`);

  return (
    <div className="flex flex-col h-full">
      {/* Toolbar */}
      <div
        className="flex items-center gap-2 px-4 py-2 border-b flex-shrink-0"
        style={{ borderColor: token.colorBorderSecondary }}
      >
        <Title level={5} className="mb-0 flex-1">Lịch định kỳ</Title>
        <Tooltip title="Agent AI có thể tạo/sửa/xoá lịch qua MCP (space_recurring_*)">
          <Tag color="blue" className="!text-xs !mr-1 flex items-center gap-1">
            <RobotOutlined /> Agent-ready
          </Tag>
        </Tooltip>
        <Tooltip title="Làm mới">
          <Button
            size="small"
            icon={<ReloadOutlined />}
            loading={hook.schedulesLoading}
            onClick={() => hook.loadSchedules()}
          />
        </Tooltip>
        <Button
          type="primary"
          size="small"
          icon={<PlusOutlined />}
          onClick={() => setShowAdd(true)}
        >
          Thêm lịch
        </Button>
      </div>

      {/* Content */}
      <div className="flex-1 overflow-y-auto px-4 py-3">
        {hook.schedulesLoading && (
          <div className="flex justify-center py-8"><Spin /></div>
        )}

        {!hook.schedulesLoading && hook.schedules.length === 0 && (
          <div className="py-8">
            <Empty
              description={
                <span>
                  Chưa có lịch định kỳ.{' '}
                  <Button type="link" size="small" onClick={() => setShowAdd(true)}>
                    Tạo ngay
                  </Button>
                </span>
              }
            />
            <Alert
              type="info"
              showIcon
              icon={<RobotOutlined />}
              className="mt-4 mx-auto max-w-md"
              message="Agent AI cũng có thể tạo lịch giúp bạn"
              description={
                <span style={{ fontSize: 12 }}>
                  Nói với agent: <code>"Đặt lịch tìm giá vàng mỗi sáng 7h"</code> — agent sẽ dùng MCP tool{' '}
                  <code>space_recurring_create</code> để tạo lịch và tự tạo chat session báo cáo.
                </span>
              }
            />
          </div>
        )}

        {active.length > 0 && (
          <Section title={`Đang hoạt động (${active.length})`}>
            {active.map(s => (
              <ScheduleCard
                key={s.id}
                schedule={s}
                token={token}
                onOpen={() => setOpenDetailId(s.id)}
                onOpenChat={() => openChat(s.chat_jid)}
                onCancel={() => hook.cancelSchedule(s.id)}
              />
            ))}
          </Section>
        )}

        {paused.length > 0 && (
          <Section title={`Tạm dừng (${paused.length})`}>
            {paused.map(s => (
              <ScheduleCard
                key={s.id}
                schedule={s}
                token={token}
                onOpen={() => setOpenDetailId(s.id)}
                onOpenChat={() => openChat(s.chat_jid)}
                onCancel={() => hook.cancelSchedule(s.id)}
              />
            ))}
          </Section>
        )}

        {completed.length > 0 && (
          <Section title={`Đã kết thúc (${completed.length})`}>
            {completed.map(s => (
              <ScheduleCard
                key={s.id}
                schedule={s}
                token={token}
                onOpen={() => setOpenDetailId(s.id)}
                onOpenChat={() => openChat(s.chat_jid)}
                onCancel={() => hook.cancelSchedule(s.id)}
              />
            ))}
          </Section>
        )}
      </div>

      <ScheduleAddModal
        open={showAdd}
        onClose={() => setShowAdd(false)}
        hook={hook}
      />

      {openDetailId && (
        <ScheduleDetailPanel
          scheduleId={openDetailId}
          hook={hook}
          onClose={() => setOpenDetailId(null)}
        />
      )}
    </div>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="mb-4">
      <Text type="secondary" className="text-xs uppercase tracking-wide mb-2 block">{title}</Text>
      <div className="space-y-2">{children}</div>
    </div>
  );
}

function relativeNextRun(v: string | null): string {
  if (!v) return '';
  const diffMs = new Date(v).getTime() - Date.now();
  if (diffMs <= 0) return 'sắp chạy';
  const m = Math.floor(diffMs / 60_000);
  if (m < 60) return `sau ${m} phút`;
  const h = Math.floor(m / 60);
  if (h < 24) return `sau ${h} giờ ${m % 60 ? `${m % 60}p` : ''}`.trim();
  const d = Math.floor(h / 24);
  return `sau ${d} ngày`;
}

function ScheduleCard({
  schedule, onOpen, onOpenChat, onCancel, token,
}: {
  schedule: SpaceSchedule;
  onOpen: () => void;
  onOpenChat: () => void;
  onCancel: () => void;
  token: ReturnType<typeof theme.useToken>['token'];
}) {
  const st = STATUS_MAP[schedule.status] ?? { color: 'default', label: schedule.status };

  const formatTs = (v: string | null) => {
    if (!v) return '—';
    return new Date(v).toLocaleString('vi', {
      day: '2-digit', month: '2-digit', hour: '2-digit', minute: '2-digit',
    });
  };

  return (
    <div
      onClick={onOpen}
      className="p-3 rounded border cursor-pointer transition-colors"
      style={{
        borderColor: schedule.status === 'active' ? token.colorPrimaryBorder : token.colorBorderSecondary,
        background: schedule.status === 'active' ? token.colorPrimaryBg : token.colorFillQuaternary,
        opacity: schedule.status === 'completed' ? 0.6 : 1,
      }}
    >
      <div className="flex items-start justify-between gap-2">
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2 mb-1 flex-wrap">
            <Tag color={st.color} className="!text-xs">{st.label}</Tag>
            <Text className="text-xs" style={{ color: token.colorTextSecondary }}>
              {describeCron(schedule)}
            </Text>
            {schedule.last_status === 'success' && (
              <Tooltip title="Lần chạy gần nhất: thành công">
                <CheckCircleTwoTone twoToneColor={token.colorSuccess} style={{ fontSize: 12 }} />
              </Tooltip>
            )}
            {schedule.last_status === 'error' && (
              <Tooltip title="Lần chạy gần nhất: lỗi">
                <CloseCircleTwoTone twoToneColor={token.colorError} style={{ fontSize: 12 }} />
              </Tooltip>
            )}
          </div>
          <Text className="text-sm font-medium block truncate">{schedule.label}</Text>
          <Text type="secondary" className="text-xs block truncate">{schedule.prompt}</Text>
          <div className="flex gap-3 mt-1 flex-wrap">
            {schedule.next_run && schedule.status === 'active' && (
              <Text type="secondary" style={{ fontSize: 11 }}>
                <ClockCircleOutlined className="mr-1" />
                Lần tới: {formatTs(schedule.next_run)}
                <span className="ml-1" style={{ color: token.colorPrimary }}>
                  · {relativeNextRun(schedule.next_run)}
                </span>
              </Text>
            )}
            {schedule.last_run && (
              <Text type="secondary" style={{ fontSize: 11 }}>
                Lần cuối: {formatTs(schedule.last_run)}
              </Text>
            )}
          </div>
        </div>

        <div className="flex flex-col items-end gap-1 flex-shrink-0">
          <Tooltip title="Mở chat session">
            <Button
              type="text" size="small" icon={<MessageOutlined />}
              onClick={e => { e.stopPropagation(); onOpenChat(); }}
            />
          </Tooltip>
          {schedule.status !== 'completed' && (
            <Popconfirm
              title="Xoá lịch này? Cả chat session đi kèm sẽ bị xoá."
              onConfirm={(e) => { e?.stopPropagation(); onCancel(); }}
              onCancel={(e) => e?.stopPropagation()}
              okText="Xoá"
              cancelText="Không"
            >
              <Button
                type="text" size="small" danger icon={<DeleteOutlined />}
                onClick={e => e.stopPropagation()}
              />
            </Popconfirm>
          )}
        </div>
      </div>
    </div>
  );
}

// ─── Add modal ────────────────────────────────────────────────────────────────

interface AddProps {
  open: boolean;
  onClose: () => void;
  hook: UseSpaceHook;
}

function ScheduleAddModal({ open, onClose, hook }: AddProps) {
  const [form] = Form.useForm();
  const [saving, setSaving] = useState(false);
  const [frequency, setFrequency] = useState<'daily' | 'weekdays' | 'weekly' | 'monthly'>('daily');
  const [agentMode, setAgentMode] = useState<AgentModeType>('agent');
  const [useAdvanced, setUseAdvanced] = useState(false);

  const handleOk = async () => {
    try {
      const vals = await form.validateFields();
      setSaving(true);
      const payload: ScheduleCreatePayload = useAdvanced
        ? { prompt: vals.prompt, label: vals.label, cron_advanced: vals.cron_advanced, agent_mode: agentMode }
        : {
            prompt: vals.prompt,
            label: vals.label,
            time_local: (vals.time as Dayjs).format('HH:mm'),
            frequency,
            weekday: frequency === 'weekly' ? vals.weekday : undefined,
            day_of_month: frequency === 'monthly' ? vals.day_of_month : undefined,
            agent_mode: agentMode,
          };
      const res = await hook.createSchedule(payload);
      if (res) {
        form.resetFields();
        setFrequency('daily');
        setAgentMode('agent');
        setUseAdvanced(false);
        onClose();
      }
    } catch {
      /* validation error */
    } finally {
      setSaving(false);
    }
  };

  return (
    <Modal
      title="Thêm lịch định kỳ"
      open={open}
      onCancel={() => { form.resetFields(); setUseAdvanced(false); onClose(); }}
      onOk={handleOk}
      okText="Tạo"
      cancelText="Huỷ"
      confirmLoading={saving}
      destroyOnClose
    >
      <Form
        form={form}
        layout="vertical"
        initialValues={{
          time: dayjs().hour(7).minute(0),
          weekday: 1,
          day_of_month: 1,
        }}
      >
        <Form.Item
          name="label"
          label="Tên gọi (tuỳ chọn)"
          tooltip="Hiển thị trên thẻ và làm tên chat session"
        >
          <Input placeholder="VD: Báo giá vàng sáng" />
        </Form.Item>

        <Form.Item
          name="prompt"
          label="Yêu cầu cho agent"
          rules={[{ required: true, message: 'Nhập nội dung' }]}
        >
          <TextArea
            rows={3}
            placeholder={
              'VD: Tìm giá vàng SJC hôm nay và tóm tắt biến động so với hôm qua\n' +
              'VD: Liệt kê email chưa đọc và phân loại theo độ ưu tiên'
            }
          />
        </Form.Item>

        <Form.Item
          label="Chế độ chạy"
          tooltip="Agent: chạy đơn lẻ. DAG: chạy nhóm agent theo sơ đồ. Plan: lên kế hoạch rồi thực thi."
        >
          <Radio.Group
            optionType="button"
            buttonStyle="solid"
            value={agentMode}
            onChange={e => setAgentMode(e.target.value)}
            options={[
              { label: 'Agent', value: 'agent' },
              { label: 'DAG', value: 'dag' },
              { label: 'Plan', value: 'plan' },
            ]}
          />
        </Form.Item>

        {!useAdvanced && (
          <>
            <Form.Item label="Tần suất" required>
              <Radio.Group
                optionType="button"
                buttonStyle="solid"
                value={frequency}
                onChange={e => setFrequency(e.target.value)}
                options={[
                  { label: 'Mỗi ngày', value: 'daily' },
                  { label: 'Th 2–6', value: 'weekdays' },
                  { label: 'Hàng tuần', value: 'weekly' },
                  { label: 'Hàng tháng', value: 'monthly' },
                ]}
              />
            </Form.Item>

            <Form.Item
              name="time"
              label="Giờ chạy (theo giờ máy)"
              rules={[{ required: true, message: 'Chọn giờ' }]}
            >
              <TimePicker format="HH:mm" minuteStep={5} style={{ width: 140 }} />
            </Form.Item>

            {frequency === 'weekly' && (
              <Form.Item name="weekday" label="Thứ trong tuần" rules={[{ required: true }]}>
                <Radio.Group optionType="button" options={WEEKDAY_OPTS} />
              </Form.Item>
            )}

            {frequency === 'monthly' && (
              <Form.Item
                name="day_of_month"
                label="Ngày trong tháng"
                rules={[{ required: true }]}
              >
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
              <Form.Item
                name="cron_advanced"
                rules={useAdvanced ? [{ required: true, message: 'Nhập cron 5 trường' }] : []}
                tooltip="5 trường: phút giờ ngày tháng thứ. VD: 0 9 * * 1-5"
              >
                <Input placeholder="0 7 * * *" className="font-mono" />
              </Form.Item>
            ),
          }]}
        />
      </Form>
    </Modal>
  );
}
