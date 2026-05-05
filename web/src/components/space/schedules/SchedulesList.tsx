import React, { useEffect, useState } from 'react';
import {
  Button, Modal, Form, Input, Select, Tag, Empty, Spin, Popconfirm,
  Typography, theme, Tooltip, Alert, Card, Badge,
} from 'antd';
import {
  PlusOutlined, DeleteOutlined, ClockCircleOutlined, PlayCircleOutlined,
  PauseCircleOutlined, ReloadOutlined,
} from '@ant-design/icons';
import type { UseSpaceHook } from '../../../hooks/useSpace';

const { Text, Title } = Typography;
const { TextArea } = Input;

const CRON_PRESETS = [
  { label: 'Mỗi ngày 7h sáng', value: '0 7 * * *' },
  { label: 'Mỗi ngày 8h sáng', value: '0 8 * * *' },
  { label: 'Mỗi ngày 12h trưa', value: '0 12 * * *' },
  { label: 'Mỗi ngày 6h chiều', value: '0 18 * * *' },
  { label: 'Thứ 2 đầu tuần 9h', value: '0 9 * * 1' },
  { label: 'Mỗi thứ 6 5h chiều', value: '0 17 * * 5' },
  { label: 'Tùy chỉnh...', value: '__custom__' },
];

const STATUS_MAP: Record<string, { color: string; label: string }> = {
  active: { color: 'green', label: 'Đang chạy' },
  paused: { color: 'orange', label: 'Tạm dừng' },
  completed: { color: 'default', label: 'Đã hủy' },
};

interface Props {
  hook: UseSpaceHook;
  groupFolder: string;
  chatJid: string;
}

export function SchedulesList({ hook, groupFolder, chatJid }: Props) {
  const { token } = theme.useToken();
  const [showModal, setShowModal] = useState(false);
  const [form] = Form.useForm();
  const [saving, setSaving] = useState(false);
  const [customCron, setCustomCron] = useState(false);

  useEffect(() => {
    if (groupFolder) hook.loadSchedules(groupFolder);
  }, [groupFolder]);

  const handleCreate = async () => {
    try {
      const vals = await form.validateFields();
      const cron = vals.cron_preset === '__custom__' ? vals.cron_custom : vals.cron_preset;
      setSaving(true);
      await hook.createSchedule(vals.prompt, cron, groupFolder, chatJid);
      setShowModal(false);
      form.resetFields();
      setCustomCron(false);
    } catch {
      // validation error
    } finally {
      setSaving(false);
    }
  };

  const handleCancel = async (id: string) => {
    await hook.cancelSchedule(id, groupFolder);
  };

  const active = hook.schedules.filter(s => s.status === 'active');
  const others = hook.schedules.filter(s => s.status !== 'active');

  const noGroup = !groupFolder;

  return (
    <div className="flex flex-col h-full">
      {/* Toolbar */}
      <div
        className="flex items-center gap-2 px-4 py-2 border-b flex-shrink-0"
        style={{ borderColor: token.colorBorderSecondary }}
      >
        <Title level={5} className="mb-0 flex-1">Lịch định kỳ</Title>
        <Tooltip title="Làm mới">
          <Button
            size="small"
            icon={<ReloadOutlined />}
            loading={hook.schedulesLoading}
            onClick={() => hook.loadSchedules(groupFolder)}
            disabled={noGroup}
          />
        </Tooltip>
        <Button
          type="primary"
          size="small"
          icon={<PlusOutlined />}
          onClick={() => setShowModal(true)}
          disabled={noGroup}
        >
          Thêm lịch
        </Button>
      </div>

      {/* Content */}
      <div className="flex-1 overflow-y-auto px-4 py-3">
        {noGroup && (
          <Alert
            type="warning"
            title="Chưa chọn group"
            description="Chức năng lịch định kỳ yêu cầu group folder. Kiểm tra Settings → Groups."
            showIcon
            className="mb-3"
          />
        )}

        {hook.schedulesLoading && (
          <div className="flex justify-center py-8"><Spin /></div>
        )}

        {!hook.schedulesLoading && hook.schedules.length === 0 && !noGroup && (
          <Empty
            description={
              <span>
                Chưa có lịch định kỳ.{' '}
                <Button type="link" size="small" onClick={() => setShowModal(true)}>
                  Tạo ngay
                </Button>
              </span>
            }
            className="py-8"
          />
        )}

        {/* Active schedules */}
        {active.length > 0 && (
          <div className="mb-4">
            <Text type="secondary" className="text-xs uppercase tracking-wide mb-2 block">
              Đang hoạt động ({active.length})
            </Text>
            <div className="space-y-2">
              {active.map(s => (
                <ScheduleCard key={s.id} schedule={s} onCancel={handleCancel} token={token} />
              ))}
            </div>
          </div>
        )}

        {/* Completed / paused */}
        {others.length > 0 && (
          <div>
            <Text type="secondary" className="text-xs uppercase tracking-wide mb-2 block">
              Đã kết thúc ({others.length})
            </Text>
            <div className="space-y-2">
              {others.map(s => (
                <ScheduleCard key={s.id} schedule={s} onCancel={handleCancel} token={token} />
              ))}
            </div>
          </div>
        )}
      </div>

      {/* Create modal */}
      <Modal
        title="Thêm lịch định kỳ"
        open={showModal}
        onCancel={() => { setShowModal(false); form.resetFields(); setCustomCron(false); }}
        onOk={handleCreate}
        okText="Tạo"
        cancelText="Hủy"
        confirmLoading={saving}
      >
        <Alert
          type="info"
          className="mb-3"
          title="Lịch định kỳ sẽ chạy agent với nội dung prompt vào thời điểm đã đặt."
          showIcon
        />
        <Form form={form} layout="vertical" className="mt-2">
          <Form.Item
            name="prompt"
            label="Nhiệm vụ agent sẽ thực hiện"
            rules={[{ required: true, message: 'Nhập nhiệm vụ' }]}
            tooltip="Agent sẽ nhận prompt này và thực hiện mỗi lần lịch chạy"
          >
            <TextArea
              rows={3}
              placeholder="VD: Lấy giá vàng hôm nay và báo cáo vào chat&#10;VD: Kiểm tra hộp thư và tóm tắt email chưa đọc"
            />
          </Form.Item>

          <Form.Item
            name="cron_preset"
            label="Lịch lặp lại"
            rules={[{ required: true, message: 'Chọn lịch' }]}
          >
            <Select
              placeholder="Chọn thời gian..."
              options={CRON_PRESETS}
              onChange={v => setCustomCron(v === '__custom__')}
            />
          </Form.Item>

          {customCron && (
            <Form.Item
              name="cron_custom"
              label="Cron expression"
              tooltip="VD: '0 9 * * 1-5' = 9h sáng thứ 2-6"
              rules={[{ required: true, message: 'Nhập cron expression' }]}
            >
              <Input
                placeholder="0 7 * * * (phút giờ ngày tháng thứ)"
                className="font-mono"
              />
            </Form.Item>
          )}
        </Form>
      </Modal>
    </div>
  );
}

// ─── Schedule card ─────────────────────────────────────────────────────────────

interface CardProps {
  schedule: ReturnType<typeof useScheduleType>;
  onCancel: (id: string) => void;
  token: ReturnType<typeof theme.useToken>['token'];
}

// workaround for type inference
function useScheduleType() {
  return null as unknown as import('../../../hooks/useSpace').SpaceSchedule;
}

function ScheduleCard({
  schedule,
  onCancel,
  token,
}: {
  schedule: import('../../../hooks/useSpace').SpaceSchedule;
  onCancel: (id: string) => void;
  token: ReturnType<typeof theme.useToken>['token'];
}) {
  const st = STATUS_MAP[schedule.status] ?? { color: 'default', label: schedule.status };

  const formatNext = (v: string | null) => {
    if (!v) return '—';
    const d = new Date(v);
    return d.toLocaleString('vi', { day: '2-digit', month: '2-digit', hour: '2-digit', minute: '2-digit' });
  };

  return (
    <div
      className="p-3 rounded border"
      style={{
        borderColor: schedule.status === 'active' ? token.colorPrimaryBorder : token.colorBorderSecondary,
        background: schedule.status === 'active' ? token.colorPrimaryBg : token.colorFillQuaternary,
        opacity: schedule.status === 'completed' ? 0.6 : 1,
      }}
    >
      <div className="flex items-start justify-between gap-2">
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2 mb-1">
            <Tag color={st.color} className="!text-xs">{st.label}</Tag>
            <Text className="font-mono text-xs" style={{ color: token.colorTextSecondary }}>
              {schedule.schedule_value}
            </Text>
          </div>
          <Text ellipsis className="text-sm block">{schedule.prompt}</Text>
          <div className="flex gap-4 mt-1">
            {schedule.next_run && (
              <Text type="secondary" style={{ fontSize: 11 }}>
                <ClockCircleOutlined className="mr-1" />
                Lần tới: {formatNext(schedule.next_run)}
              </Text>
            )}
            {schedule.last_run && (
              <Text type="secondary" style={{ fontSize: 11 }}>
                Lần cuối: {formatNext(schedule.last_run)}
              </Text>
            )}
          </div>
        </div>

        {schedule.status === 'active' && (
          <Popconfirm
            title="Hủy lịch này?"
            onConfirm={() => onCancel(schedule.id)}
            okText="Hủy lịch"
            cancelText="Không"
          >
            <Button type="text" size="small" danger icon={<DeleteOutlined />} />
          </Popconfirm>
        )}
      </div>
    </div>
  );
}
