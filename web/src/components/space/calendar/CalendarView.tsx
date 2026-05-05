import React, { useEffect, useRef, useState, useMemo, useCallback } from 'react';
import {
  Button, Modal, Form, Input, DatePicker, InputNumber, Switch,
  Typography, Spin, Popconfirm, Tooltip, theme, Segmented, Drawer,
  Divider, Tag, Space,
} from 'antd';
import {
  PlusOutlined, DeleteOutlined, BellOutlined, LeftOutlined, RightOutlined,
  EnvironmentOutlined, ClockCircleOutlined, EditOutlined, CloseOutlined,
} from '@ant-design/icons';
import type { SpaceEvent, UseSpaceHook } from '../../../hooks/useSpace';
import dayjs, { Dayjs } from 'dayjs';
import 'dayjs/locale/vi';
import isSameOrBefore from 'dayjs/plugin/isSameOrBefore';
import isSameOrAfter from 'dayjs/plugin/isSameOrAfter';

dayjs.extend(isSameOrBefore);
dayjs.extend(isSameOrAfter);
dayjs.locale('vi');

// ─── Constants ────────────────────────────────────────────────────────────────

const HOUR_H = 56;
const HOURS = Array.from({ length: 24 }, (_, i) => i);
const DAY_NAMES = ['CN', 'T2', 'T3', 'T4', 'T5', 'T6', 'T7'];

const SOURCE_COLOR: Record<string, string> = {
  manual: '#5BBFE8',
  google: '#4285F4',
  apple: '#8E8E93',
  agent: '#9B59B6',
  cowork: '#27AE60',
  ical: '#E67E22',
};

const SOURCE_LABEL: Record<string, string> = {
  manual: 'Thủ công',
  google: 'Google Calendar',
  apple: 'Apple Calendar',
  agent: 'Agent',
  cowork: 'Cowork',
  ical: 'iCal',
};

type ViewMode = 'week' | 'month' | 'day';

const { RangePicker } = DatePicker;
const { TextArea } = Input;
const { Text, Title } = Typography;

// ─── Utils ────────────────────────────────────────────────────────────────────

function evColor(ev: SpaceEvent) {
  return ev.color ?? SOURCE_COLOR[ev.source] ?? '#5BBFE8';
}

function fmt(ms: number) {
  return dayjs(ms).format('HH:mm');
}

function fmtFull(ms: number) {
  return dayjs(ms).format('dddd, D MMMM YYYY · HH:mm');
}

function msToMinutes(ms: number) {
  const d = dayjs(ms);
  return d.hour() * 60 + d.minute();
}

// ─── Event Detail Drawer ─────────────────────────────────────────────────────

interface DetailDrawerProps {
  event: SpaceEvent | null;
  onClose: () => void;
  onEdit: (ev: SpaceEvent) => void;
  onDelete: (id: string) => void;
  token: ReturnType<typeof theme.useToken>['token'];
}

function EventDetailDrawer({ event, onClose, onEdit, onDelete, token }: DetailDrawerProps) {
  if (!event) return null;
  const color = evColor(event);

  return (
    <Drawer
      open={!!event}
      onClose={onClose}
      width={360}
      closable={false}
      styles={{ body: { padding: 0 } }}
    >
      {/* Color bar header */}
      <div
        className="flex items-start justify-between px-5 py-4"
        style={{ background: color + '18', borderBottom: `3px solid ${color}` }}
      >
        <div className="flex-1 min-w-0 pr-3">
          <Title level={5} className="!mb-1 !leading-snug" style={{ color: token.colorText }}>
            {event.title}
          </Title>
          <Tag
            style={{
              background: color + '25',
              borderColor: color + '60',
              color,
              fontSize: 11,
            }}
          >
            {SOURCE_LABEL[event.source] ?? event.source}
          </Tag>
        </div>
        <Button
          type="text"
          size="small"
          icon={<CloseOutlined />}
          onClick={onClose}
          style={{ color: token.colorTextSecondary }}
        />
      </div>

      {/* Detail body */}
      <div className="px-5 py-4 space-y-4">
        {/* Time */}
        <div className="flex items-start gap-3">
          <ClockCircleOutlined style={{ color: token.colorTextSecondary, marginTop: 3 }} />
          <div>
            {event.all_day ? (
              <Text>{dayjs(event.start_at).format('dddd, D MMMM YYYY')} · Cả ngày</Text>
            ) : (
              <>
                <Text className="block">{dayjs(event.start_at).format('dddd, D MMMM YYYY')}</Text>
                <Text type="secondary" className="text-sm">
                  {fmt(event.start_at)} – {fmt(event.end_at)}
                  {' '}
                  <span style={{ color: token.colorTextQuaternary }}>
                    ({Math.round((event.end_at - event.start_at) / 60000)} phút)
                  </span>
                </Text>
              </>
            )}
          </div>
        </div>

        {/* Location */}
        {event.location && (
          <div className="flex items-start gap-3">
            <EnvironmentOutlined style={{ color: token.colorTextSecondary, marginTop: 3 }} />
            <Text>{event.location}</Text>
          </div>
        )}

        {/* Reminder */}
        {event.reminder_min != null && (
          <div className="flex items-center gap-3">
            <BellOutlined style={{ color: token.colorWarning }} />
            <Text>Nhắc trước <strong>{event.reminder_min}</strong> phút</Text>
          </div>
        )}

        {/* Description */}
        {event.description && (
          <>
            <Divider className="!my-2" />
            <Text className="text-sm whitespace-pre-wrap" style={{ color: token.colorText }}>
              {event.description}
            </Text>
          </>
        )}
      </div>

      {/* Actions */}
      <div
        className="absolute bottom-0 left-0 right-0 flex gap-2 px-5 py-3 border-t"
        style={{ borderColor: token.colorBorderSecondary, background: token.colorBgContainer }}
      >
        <Button
          icon={<EditOutlined />}
          onClick={() => { onClose(); onEdit(event); }}
          style={{ flex: 1 }}
        >
          Chỉnh sửa
        </Button>
        <Popconfirm
          title="Xóa sự kiện này?"
          description="Hành động không thể hoàn tác."
          onConfirm={() => { onDelete(event.id); onClose(); }}
          okText="Xóa"
          cancelText="Hủy"
          okButtonProps={{ danger: true }}
        >
          <Button danger icon={<DeleteOutlined />} style={{ flex: 1 }}>
            Xóa
          </Button>
        </Popconfirm>
      </div>
    </Drawer>
  );
}

// ─── Props ────────────────────────────────────────────────────────────────────

interface Props { hook: UseSpaceHook }

// ─── Main component ───────────────────────────────────────────────────────────

export function CalendarView({ hook }: Props) {
  const { token } = theme.useToken();
  const [view, setView] = useState<ViewMode>('week');
  const [cursor, setCursor] = useState<Dayjs>(dayjs());

  // Create / edit modal
  const [showModal, setShowModal] = useState(false);
  const [editingEvent, setEditingEvent] = useState<SpaceEvent | null>(null);
  const [form] = Form.useForm();
  const [saving, setSaving] = useState(false);

  // Detail drawer
  const [detailEvent, setDetailEvent] = useState<SpaceEvent | null>(null);

  const scrollRef = useRef<HTMLDivElement>(null);

  // ── Range computation ──────────────────────────────────────────────────────

  const { rangeStart, rangeEnd, weekDays, monthGrid } = useMemo(() => {
    if (view === 'day') {
      const start = cursor.startOf('day');
      const end = cursor.endOf('day');
      return { rangeStart: start, rangeEnd: end, weekDays: [cursor], monthGrid: [] };
    }
    if (view === 'week') {
      const start = cursor.startOf('week');
      const end = start.add(6, 'day').endOf('day');
      const weekDays = Array.from({ length: 7 }, (_, i) => start.add(i, 'day'));
      return { rangeStart: start, rangeEnd: end, weekDays, monthGrid: [] };
    }
    const mStart = cursor.startOf('month');
    const mEnd = cursor.endOf('month');
    const gridStart = mStart.startOf('week');
    const gridEnd = mEnd.endOf('week');
    const total = gridEnd.diff(gridStart, 'day') + 1;
    const cells = Array.from({ length: total }, (_, i) => gridStart.add(i, 'day'));
    const weeks: Dayjs[][] = [];
    for (let i = 0; i < cells.length; i += 7) weeks.push(cells.slice(i, i + 7));
    return { rangeStart: gridStart, rangeEnd: gridEnd, weekDays: [], monthGrid: weeks };
  }, [view, cursor.valueOf()]);

  useEffect(() => {
    hook.loadEvents(rangeStart.valueOf(), rangeEnd.valueOf());
  }, [rangeStart.valueOf(), rangeEnd.valueOf()]);

  useEffect(() => {
    if ((view === 'week' || view === 'day') && scrollRef.current) {
      const now = dayjs();
      const top = Math.max(0, (now.hour() - 1) * HOUR_H);
      scrollRef.current.scrollTop = top;
    }
  }, [view]);

  // ── Navigation ─────────────────────────────────────────────────────────────

  const nav = (dir: -1 | 1) => {
    setCursor(prev =>
      view === 'day' ? prev.add(dir, 'day')
        : view === 'week' ? prev.add(dir * 7, 'day')
          : prev.add(dir, 'month')
    );
  };

  // ── Event grouping ─────────────────────────────────────────────────────────

  const eventsByDay = useMemo(() => {
    const map: Record<string, SpaceEvent[]> = {};
    for (const ev of hook.events) {
      const key = dayjs(ev.start_at).format('YYYY-MM-DD');
      (map[key] ??= []).push(ev);
    }
    return map;
  }, [hook.events]);

  // ── Open create / edit modal ───────────────────────────────────────────────

  const openCreate = (defaultDay?: Dayjs) => {
    setEditingEvent(null);
    form.resetFields();
    if (defaultDay) {
      form.setFieldValue('range', [
        defaultDay.hour(9).minute(0),
        defaultDay.hour(10).minute(0),
      ]);
    }
    setShowModal(true);
  };

  const openEdit = (ev: SpaceEvent) => {
    setEditingEvent(ev);
    form.setFieldsValue({
      title: ev.title,
      range: [dayjs(ev.start_at), dayjs(ev.end_at)],
      all_day: ev.all_day,
      location: ev.location ?? undefined,
      description: ev.description ?? undefined,
      reminder_min: ev.reminder_min ?? undefined,
    });
    setShowModal(true);
  };

  const handleSave = async () => {
    try {
      const vals = await form.validateFields();
      setSaving(true);
      const [start, end]: [Dayjs, Dayjs] = vals.range;
      const payload = {
        title: vals.title,
        description: vals.description ?? null,
        start_at: start.valueOf(),
        end_at: end.valueOf(),
        all_day: vals.all_day ?? false,
        location: vals.location ?? null,
        color: editingEvent?.color ?? null,
        reminder_min: vals.reminder_min ?? null,
      };

      if (editingEvent) {
        await hook.updateEvent(editingEvent.id, payload);
      } else {
        await hook.createEvent({ ...payload, source: 'manual' } as any);
      }
      setShowModal(false);
      form.resetFields();
      setEditingEvent(null);
    } catch {
      // stay open on validation error
    } finally {
      setSaving(false);
    }
  };

  // ── Header label ───────────────────────────────────────────────────────────

  const headerLabel =
    view === 'day' ? cursor.format('dddd, D MMMM YYYY')
      : view === 'week' ? `${rangeStart.format('D MMM')} – ${rangeEnd.format('D MMM YYYY')}`
        : cursor.format('MMMM YYYY');

  const today = dayjs().format('YYYY-MM-DD');

  return (
    <div className="flex flex-col h-full select-none">

      {/* ── Toolbar ─────────────────────────────────────────────────────────── */}
      <div
        className="flex items-center gap-3 px-4 py-2.5 border-b flex-shrink-0"
        style={{ borderColor: token.colorBorderSecondary, background: token.colorBgContainer }}
      >
        <div className="flex items-center gap-1">
          <Button size="small" type="text" shape="circle" icon={<LeftOutlined />} onClick={() => nav(-1)} />
          <Button size="small" type="default" onClick={() => setCursor(dayjs())} style={{ minWidth: 64, fontSize: 12 }}>
            Hôm nay
          </Button>
          <Button size="small" type="text" shape="circle" icon={<RightOutlined />} onClick={() => nav(1)} />
        </div>

        <span className="font-semibold text-sm flex-1" style={{ color: token.colorText }}>
          {headerLabel}
        </span>

        <Segmented
          size="small"
          value={view}
          onChange={v => setView(v as ViewMode)}
          options={[
            { label: 'Ngày', value: 'day' },
            { label: 'Tuần', value: 'week' },
            { label: 'Tháng', value: 'month' },
          ]}
        />

        <Button type="primary" size="small" icon={<PlusOutlined />} onClick={() => openCreate()}>
          Thêm sự kiện
        </Button>
      </div>

      {/* ── Loading ──────────────────────────────────────────────────────────── */}
      {hook.eventsLoading && (
        <div className="flex justify-center py-12"><Spin size="large" /></div>
      )}

      {/* ── Week / Day timeline ──────────────────────────────────────────────── */}
      {!hook.eventsLoading && (view === 'week' || view === 'day') && (
        <WeekTimeline
          days={view === 'day' ? [cursor] : weekDays}
          today={today}
          eventsByDay={eventsByDay}
          hook={hook}
          token={token}
          scrollRef={scrollRef}
          onAddDay={openCreate}
          onSelectEvent={setDetailEvent}
        />
      )}

      {/* ── Month grid ───────────────────────────────────────────────────────── */}
      {!hook.eventsLoading && view === 'month' && (
        <MonthGrid
          cursor={cursor}
          weeks={monthGrid}
          today={today}
          eventsByDay={eventsByDay}
          hook={hook}
          token={token}
          onAddDay={openCreate}
          onSelectEvent={setDetailEvent}
        />
      )}

      {/* ── Event detail drawer ──────────────────────────────────────────────── */}
      <EventDetailDrawer
        event={detailEvent}
        onClose={() => setDetailEvent(null)}
        onEdit={ev => { setDetailEvent(null); openEdit(ev); }}
        onDelete={id => { hook.deleteEvent(id); }}
        token={token}
      />

      {/* ── Create / Edit modal ──────────────────────────────────────────────── */}
      <Modal
        title={
          <div className="flex items-center gap-2">
            <span className="w-3 h-3 rounded-full inline-block" style={{ background: token.colorPrimary }} />
            {editingEvent ? 'Chỉnh sửa sự kiện' : 'Tạo sự kiện mới'}
          </div>
        }
        open={showModal}
        onCancel={() => { setShowModal(false); form.resetFields(); setEditingEvent(null); }}
        onOk={handleSave}
        okText={editingEvent ? 'Lưu thay đổi' : 'Tạo sự kiện'}
        cancelText="Hủy"
        confirmLoading={saving}
        width={480}
      >
        <Form form={form} layout="vertical" className="mt-3">
          <Form.Item name="title" rules={[{ required: true, message: 'Nhập tiêu đề sự kiện' }]}>
            <Input
              size="large"
              placeholder="Tiêu đề sự kiện..."
              bordered={false}
              className="text-lg font-medium px-0"
              style={{ borderBottom: `2px solid ${token.colorPrimary}`, borderRadius: 0 }}
            />
          </Form.Item>

          <Form.Item name="range" rules={[{ required: true, message: 'Chọn thời gian' }]}>
            <RangePicker
              showTime={{ format: 'HH:mm' }}
              format="ddd DD/MM · HH:mm"
              style={{ width: '100%' }}
              placeholder={['Bắt đầu', 'Kết thúc']}
            />
          </Form.Item>

          <Form.Item name="all_day" valuePropName="checked">
            <div className="flex items-center gap-2">
              <Switch size="small" />
              <Text className="text-sm">Sự kiện cả ngày</Text>
            </div>
          </Form.Item>

          <Form.Item name="location">
            <Input
              prefix={<EnvironmentOutlined style={{ color: token.colorTextSecondary }} />}
              placeholder="Địa điểm (tuỳ chọn)"
            />
          </Form.Item>

          <Form.Item name="description">
            <TextArea rows={2} placeholder="Mô tả (tuỳ chọn)" />
          </Form.Item>

          <Form.Item name="reminder_min" label={
            <span className="flex items-center gap-1"><BellOutlined /> Nhắc nhở trước</span>
          }>
            <InputNumber
              min={1} max={1440}
              placeholder="Không nhắc nhở"
              addonAfter="phút"
              style={{ width: '100%' }}
            />
          </Form.Item>
        </Form>
      </Modal>
    </div>
  );
}

// ─── Week / Day Timeline ──────────────────────────────────────────────────────

interface TimelineProps {
  days: Dayjs[];
  today: string;
  eventsByDay: Record<string, SpaceEvent[]>;
  hook: UseSpaceHook;
  token: ReturnType<typeof theme.useToken>['token'];
  scrollRef: React.RefObject<HTMLDivElement>;
  onAddDay: (d: Dayjs) => void;
  onSelectEvent: (ev: SpaceEvent) => void;
}

function WeekTimeline({ days, today, eventsByDay, hook, token, scrollRef, onAddDay, onSelectEvent }: TimelineProps) {
  const nowMin = dayjs().hour() * 60 + dayjs().minute();

  return (
    <div className="flex flex-col flex-1 min-h-0">
      {/* Day header row */}
      <div
        className="flex border-b flex-shrink-0"
        style={{ borderColor: token.colorBorderSecondary, background: token.colorBgContainer }}
      >
        <div className="w-14 flex-shrink-0" />
        {days.map(d => {
          const key = d.format('YYYY-MM-DD');
          const isToday = key === today;
          return (
            <div
              key={key}
              className="flex-1 flex flex-col items-center py-2 border-l cursor-pointer group"
              style={{ borderColor: token.colorBorderSecondary }}
              onClick={() => onAddDay(d)}
            >
              <span
                className="text-xs font-medium uppercase tracking-wide"
                style={{ color: isToday ? token.colorPrimary : token.colorTextSecondary }}
              >
                {DAY_NAMES[d.day()]}
              </span>
              <span
                className="text-xl font-bold mt-0.5 w-9 h-9 flex items-center justify-center rounded-full transition-colors"
                style={{
                  background: isToday ? token.colorPrimary : 'transparent',
                  color: isToday ? '#fff' : token.colorText,
                }}
              >
                {d.format('D')}
              </span>
            </div>
          );
        })}
      </div>

      <AllDayStrip days={days} eventsByDay={eventsByDay} hook={hook} token={token} onSelectEvent={onSelectEvent} />

      <div ref={scrollRef} className="flex-1 overflow-y-auto relative">
        <div className="flex" style={{ minHeight: HOUR_H * 24 }}>
          {/* Hour labels */}
          <div className="w-14 flex-shrink-0 relative">
            {HOURS.map(h => (
              <div
                key={h}
                className="absolute flex items-start justify-end pr-2"
                style={{ top: h * HOUR_H - 8, height: HOUR_H, width: '100%' }}
              >
                {h > 0 && (
                  <span className="text-xs" style={{ color: token.colorTextQuaternary, lineHeight: 1 }}>
                    {String(h).padStart(2, '0')}:00
                  </span>
                )}
              </div>
            ))}
          </div>

          {/* Day columns */}
          {days.map(d => {
            const key = d.format('YYYY-MM-DD');
            const isToday = key === today;
            const dayEvs = (eventsByDay[key] ?? []).filter(ev => !ev.all_day);

            return (
              <div key={key} className="flex-1 relative border-l" style={{ borderColor: token.colorBorderSecondary }}>
                {HOURS.map(h => (
                  <div
                    key={h}
                    className="absolute w-full border-t"
                    style={{ top: h * HOUR_H, borderColor: token.colorBorderSecondary, opacity: 0.6 }}
                  />
                ))}

                {isToday && (
                  <div className="absolute inset-0 pointer-events-none" style={{ background: token.colorPrimary + '06' }} />
                )}

                {dayEvs.map(ev => (
                  <TimelineEvent key={ev.id} event={ev} hook={hook} token={token} onSelect={onSelectEvent} />
                ))}

                {isToday && (
                  <div
                    className="absolute left-0 right-0 z-10 flex items-center pointer-events-none"
                    style={{ top: (nowMin / 60) * HOUR_H }}
                  >
                    <div className="w-2.5 h-2.5 rounded-full flex-shrink-0 -ml-1.5" style={{ background: '#f5222d' }} />
                    <div className="flex-1 border-t" style={{ borderColor: '#f5222d', borderWidth: 1.5 }} />
                  </div>
                )}
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}

// ─── All-day strip ────────────────────────────────────────────────────────────

function AllDayStrip({
  days, eventsByDay, hook, token, onSelectEvent,
}: Pick<TimelineProps, 'days' | 'eventsByDay' | 'hook' | 'token' | 'onSelectEvent'>) {
  const hasAny = days.some(d => (eventsByDay[d.format('YYYY-MM-DD')] ?? []).some(e => e.all_day));
  if (!hasAny) return null;

  return (
    <div className="flex border-b flex-shrink-0" style={{ borderColor: token.colorBorderSecondary, minHeight: 28 }}>
      <div className="w-14 flex-shrink-0 flex items-center justify-end pr-2">
        <span className="text-xs" style={{ color: token.colorTextQuaternary }}>Cả ngày</span>
      </div>
      {days.map(d => {
        const key = d.format('YYYY-MM-DD');
        const evs = (eventsByDay[key] ?? []).filter(e => e.all_day);
        return (
          <div key={key} className="flex-1 border-l px-0.5 py-0.5 space-y-0.5" style={{ borderColor: token.colorBorderSecondary }}>
            {evs.map(ev => (
              <div
                key={ev.id}
                className="rounded px-1.5 py-0.5 text-xs font-medium truncate cursor-pointer hover:opacity-80"
                style={{ background: evColor(ev) + '30', borderLeft: `3px solid ${evColor(ev)}`, color: token.colorText }}
                onClick={e => { e.stopPropagation(); onSelectEvent(ev); }}
              >
                {ev.title}
              </div>
            ))}
          </div>
        );
      })}
    </div>
  );
}

// ─── Timeline event block ─────────────────────────────────────────────────────

interface TEProps {
  event: SpaceEvent;
  hook: UseSpaceHook;
  token: ReturnType<typeof theme.useToken>['token'];
  onSelect: (ev: SpaceEvent) => void;
}

function TimelineEvent({ event, hook, token, onSelect }: TEProps) {
  const startMin = msToMinutes(event.start_at);
  const endMin = msToMinutes(event.end_at);
  const durationMin = Math.max(endMin - startMin, 30);
  const top = (startMin / 60) * HOUR_H;
  const height = (durationMin / 60) * HOUR_H;
  const color = evColor(event);

  return (
    <div
      className="absolute left-1 right-1 rounded-md overflow-hidden cursor-pointer group transition-all hover:shadow-md"
      style={{
        top,
        height: Math.max(height, 22),
        background: color + '20',
        border: `1px solid ${color}50`,
        borderLeft: `3px solid ${color}`,
        zIndex: 1,
      }}
      onClick={() => onSelect(event)}
    >
      <div className="px-1.5 py-0.5">
        <div className="text-xs font-semibold truncate" style={{ color }}>
          {event.title}
        </div>
        {height >= 44 && (
          <div className="text-xs truncate" style={{ color: token.colorTextSecondary }}>
            {fmt(event.start_at)} – {fmt(event.end_at)}
          </div>
        )}
        {height >= 64 && event.location && (
          <div className="text-xs flex items-center gap-0.5 mt-0.5" style={{ color: token.colorTextTertiary }}>
            <EnvironmentOutlined style={{ fontSize: 9 }} /> {event.location}
          </div>
        )}
      </div>
      {event.reminder_min != null && (
        <BellOutlined
          className="absolute bottom-1 right-1 opacity-60"
          style={{ fontSize: 9, color }}
        />
      )}
    </div>
  );
}

// ─── Month grid ───────────────────────────────────────────────────────────────

interface MonthGridProps {
  cursor: Dayjs;
  weeks: Dayjs[][];
  today: string;
  eventsByDay: Record<string, SpaceEvent[]>;
  hook: UseSpaceHook;
  token: ReturnType<typeof theme.useToken>['token'];
  onAddDay: (d: Dayjs) => void;
  onSelectEvent: (ev: SpaceEvent) => void;
}

function MonthGrid({ cursor, weeks, today, eventsByDay, hook, token, onAddDay, onSelectEvent }: MonthGridProps) {
  const currentMonth = cursor.month();

  return (
    <div className="flex flex-col flex-1 min-h-0">
      <div
        className="grid grid-cols-7 border-b flex-shrink-0"
        style={{ borderColor: token.colorBorderSecondary, background: token.colorBgContainer }}
      >
        {DAY_NAMES.map(name => (
          <div key={name} className="text-center py-2 text-xs font-semibold uppercase tracking-wide" style={{ color: token.colorTextSecondary }}>
            {name}
          </div>
        ))}
      </div>

      <div className="flex-1 overflow-y-auto">
        {weeks.map((week, wi) => (
          <div
            key={wi}
            className="grid grid-cols-7 border-b"
            style={{ borderColor: token.colorBorderSecondary, minHeight: 100 }}
          >
            {week.map(day => {
              const key = day.format('YYYY-MM-DD');
              const isToday = key === today;
              const isCurrentMonth = day.month() === currentMonth;
              const dayEvs = eventsByDay[key] ?? [];
              const visible = dayEvs.slice(0, 3);
              const more = dayEvs.length - visible.length;

              return (
                <div
                  key={key}
                  className="border-l p-1 cursor-pointer group relative"
                  style={{
                    borderColor: token.colorBorderSecondary,
                    background: isToday ? token.colorPrimary + '08' : !isCurrentMonth ? token.colorFillQuaternary : undefined,
                  }}
                  onClick={() => onAddDay(day)}
                >
                  <div className="flex justify-end mb-1">
                    <span
                      className="text-xs font-semibold w-6 h-6 flex items-center justify-center rounded-full"
                      style={{
                        background: isToday ? token.colorPrimary : 'transparent',
                        color: isToday ? '#fff' : !isCurrentMonth ? token.colorTextQuaternary : token.colorText,
                      }}
                    >
                      {day.date()}
                    </span>
                  </div>

                  <div className="space-y-0.5">
                    {visible.map(ev => (
                      <MonthEventChip key={ev.id} event={ev} token={token} onSelect={onSelectEvent} />
                    ))}
                    {more > 0 && (
                      <div className="text-xs px-1" style={{ color: token.colorTextSecondary }}>
                        +{more} sự kiện
                      </div>
                    )}
                  </div>
                </div>
              );
            })}
          </div>
        ))}
      </div>
    </div>
  );
}

// ─── Month event chip ─────────────────────────────────────────────────────────

function MonthEventChip({
  event, token, onSelect,
}: { event: SpaceEvent; token: ReturnType<typeof theme.useToken>['token']; onSelect: (ev: SpaceEvent) => void }) {
  const color = evColor(event);
  return (
    <div
      className="flex items-center gap-1 rounded px-1 py-0.5 cursor-pointer truncate hover:opacity-80 transition-opacity"
      style={{
        background: event.all_day ? color + '25' : 'transparent',
        borderLeft: `2.5px solid ${color}`,
      }}
      onClick={e => { e.stopPropagation(); onSelect(event); }}
    >
      {!event.all_day && (
        <span className="text-xs flex-shrink-0 font-medium" style={{ color }}>
          {fmt(event.start_at)}
        </span>
      )}
      <span className="text-xs truncate" style={{ color: token.colorText, fontWeight: event.all_day ? 600 : 400 }}>
        {event.title}
      </span>
      {event.reminder_min != null && (
        <BellOutlined style={{ fontSize: 9, color: token.colorWarning, flexShrink: 0 }} />
      )}
    </div>
  );
}
