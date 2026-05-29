import React, { useEffect, useRef, useState, useMemo } from 'react';
import {
  Button, Modal, Form, Input, DatePicker, InputNumber, Switch,
  Typography, Spin, Popconfirm, Tooltip, theme, Segmented, Drawer,
  Divider, Tag, Badge,
} from 'antd';
import {
  PlusOutlined, DeleteOutlined, BellOutlined, LeftOutlined, RightOutlined,
  EnvironmentOutlined, ClockCircleOutlined, EditOutlined, CloseOutlined,
  ReloadOutlined, CalendarOutlined, CheckCircleOutlined, SyncOutlined,
  MinusCircleOutlined,
} from '@ant-design/icons';
import type { SpaceEvent, UseSpaceHook } from '../../../hooks/useSpace';
import { useAppContext } from '../../../contexts/AppContext';
import dayjs, { Dayjs } from 'dayjs';
import isSameOrBefore from 'dayjs/plugin/isSameOrBefore';
import isSameOrAfter from 'dayjs/plugin/isSameOrAfter';

dayjs.extend(isSameOrBefore);
dayjs.extend(isSameOrAfter);

// ─── Constants ────────────────────────────────────────────────────────────────

const HOUR_H = 56;
const HOURS = Array.from({ length: 24 }, (_, i) => i);
const DAY_NAMES = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];
const DAY_NAMES_FULL = ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday'];
const MONTH_NAMES = [
  'January', 'February', 'March', 'April', 'May', 'June',
  'July', 'August', 'September', 'October', 'November', 'December',
];

const SOURCE_COLOR: Record<string, string> = {
  manual: '#5BBFE8',
  google: '#4285F4',
  apple: '#8E8E93',
  agent: '#9B59B6',
  cowork: '#27AE60',
  ical: '#E67E22',
};

const SOURCE_LABEL: Record<string, string> = {
  manual: 'Manual',
  google: 'Google Calendar',
  apple: 'Apple Calendar',
  agent: 'Agent',
  cowork: 'Cowork',
  ical: 'iCal',
};

const STATUS_CONFIG: Record<string, { color: string; label: string; icon: React.ReactNode }> = {
  upcoming: { color: '#1677ff', label: 'Upcoming', icon: <CalendarOutlined /> },
  ongoing:  { color: '#52c41a', label: 'Ongoing',  icon: <SyncOutlined spin /> },
  done:     { color: '#8c8c8c', label: 'Done',     icon: <CheckCircleOutlined /> },
  cancelled:{ color: '#ff4d4f', label: 'Cancelled',icon: <MinusCircleOutlined /> },
};

type ViewMode = 'week' | 'month' | 'day';

const { RangePicker } = DatePicker;
const { TextArea } = Input;
const { Text, Title } = Typography;

// ─── Utils ────────────────────────────────────────────────────────────────────

function evColor(ev: SpaceEvent) {
  if (ev.status === 'done' || ev.status === 'cancelled') return '#8c8c8c';
  return ev.color ?? SOURCE_COLOR[ev.source] ?? '#5BBFE8';
}

function fmt(ms: number) {
  const d = dayjs(ms);
  return `${String(d.hour()).padStart(2,'0')}:${String(d.minute()).padStart(2,'0')}`;
}

function fmtDate(ms: number) {
  const d = dayjs(ms);
  return `${DAY_NAMES_FULL[d.day()]}, ${MONTH_NAMES[d.month()]} ${d.date()}, ${d.year()}`;
}

function msToMinutes(ms: number) {
  const d = dayjs(ms);
  return d.hour() * 60 + d.minute();
}

function durationLabel(startMs: number, endMs: number) {
  const mins = Math.round((endMs - startMs) / 60000);
  if (mins < 60) return `${mins} min`;
  const h = Math.floor(mins / 60);
  const m = mins % 60;
  return m === 0 ? `${h} hr` : `${h} hr ${m} min`;
}

// ─── Status Badge ─────────────────────────────────────────────────────────────

function StatusBadge({ status }: { status: string }) {
  const cfg = STATUS_CONFIG[status] ?? STATUS_CONFIG.upcoming;
  return (
    <Tag
      icon={cfg.icon}
      style={{
        color: cfg.color,
        borderColor: cfg.color + '60',
        background: cfg.color + '15',
        fontSize: 11,
        lineHeight: '18px',
      }}
    >
      {cfg.label}
    </Tag>
  );
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
        style={{ background: color + '15', borderBottom: `3px solid ${color}` }}
      >
        <div className="flex-1 min-w-0 pr-3">
          <Title level={5} className="!mb-2 !leading-snug" style={{ color: token.colorText }}>
            {event.title}
          </Title>
          <div className="flex flex-wrap gap-1.5">
            <StatusBadge status={event.status ?? 'upcoming'} />
            <Tag
              style={{
                background: color + '20',
                borderColor: color + '50',
                color,
                fontSize: 11,
              }}
            >
              {SOURCE_LABEL[event.source] ?? event.source}
            </Tag>
          </div>
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
              <Text>{fmtDate(event.start_at)} · All day</Text>
            ) : (
              <>
                <Text className="block">{fmtDate(event.start_at)}</Text>
                <Text type="secondary" className="text-sm">
                  {fmt(event.start_at)} – {fmt(event.end_at)}
                  {' '}
                  <span style={{ color: token.colorTextQuaternary }}>
                    ({durationLabel(event.start_at, event.end_at)})
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
            <Text>
              Reminder <strong>{event.reminder_min}</strong> min before
            </Text>
          </div>
        )}

        {/* Re-notification */}
        {event.renotify_min != null && (
          <div className="flex items-center gap-3">
            <ReloadOutlined style={{ color: token.colorPrimary }} />
            <Text>
              Re-notify every <strong>{event.renotify_min}</strong> min while ongoing
            </Text>
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
          Edit
        </Button>
        <Popconfirm
          title="Delete this event?"
          description="This action cannot be undone."
          onConfirm={() => { onDelete(event.id); onClose(); }}
          okText="Delete"
          cancelText="Cancel"
          okButtonProps={{ danger: true }}
        >
          <Button danger icon={<DeleteOutlined />} style={{ flex: 1 }}>
            Delete
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

  const [showModal, setShowModal] = useState(false);
  const [editingEvent, setEditingEvent] = useState<SpaceEvent | null>(null);
  const [form] = Form.useForm();
  const [saving, setSaving] = useState(false);

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

  // Reload when the backend signals a Space mutation from elsewhere
  // (typically the chat agent calling `mcp__senclaw-space__event_create/
  // update/delete`). Without this kick, agent-driven changes land in
  // the DB but the calendar keeps showing the stale snapshot until the
  // user manually navigates to a different week.
  const { ws } = useAppContext();
  useEffect(() => {
    if (ws.spaceEventsRev > 0) {
      hook.loadEvents(rangeStart.valueOf(), rangeEnd.valueOf());
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ws.spaceEventsRev]);

  useEffect(() => {
    if ((view === 'week' || view === 'day') && scrollRef.current) {
      const top = Math.max(0, (dayjs().hour() - 1) * HOUR_H);
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
      renotify_min: ev.renotify_min ?? undefined,
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
        renotify_min: vals.renotify_min ?? null,
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

  const headerLabel = useMemo(() => {
    if (view === 'day') {
      return `${DAY_NAMES_FULL[cursor.day()]}, ${MONTH_NAMES[cursor.month()]} ${cursor.date()}, ${cursor.year()}`;
    }
    if (view === 'week') {
      const s = rangeStart;
      const e = rangeEnd;
      if (s.month() === e.month()) {
        return `${MONTH_NAMES[s.month()]} ${s.date()} – ${e.date()}, ${e.year()}`;
      }
      return `${MONTH_NAMES[s.month()]} ${s.date()} – ${MONTH_NAMES[e.month()]} ${e.date()}, ${e.year()}`;
    }
    return `${MONTH_NAMES[cursor.month()]} ${cursor.year()}`;
  }, [view, cursor.valueOf(), rangeStart.valueOf(), rangeEnd.valueOf()]);

  const today = dayjs().format('YYYY-MM-DD');

  // ── Ongoing event count for toolbar badge ──────────────────────────────────
  const ongoingCount = hook.events.filter(e => e.status === 'ongoing').length;

  return (
    <div className="flex flex-col h-full select-none">

      {/* ── Toolbar ─────────────────────────────────────────────────────────── */}
      <div
        className="flex items-center gap-3 px-4 py-2.5 border-b flex-shrink-0"
        style={{ borderColor: token.colorBorderSecondary, background: token.colorBgContainer }}
      >
        <div className="flex items-center gap-1">
          <Button size="small" type="text" shape="circle" icon={<LeftOutlined />} onClick={() => nav(-1)} />
          <Button size="small" type="default" onClick={() => setCursor(dayjs())} style={{ minWidth: 56, fontSize: 12 }}>
            Today
          </Button>
          <Button size="small" type="text" shape="circle" icon={<RightOutlined />} onClick={() => nav(1)} />
        </div>

        <span className="font-semibold text-sm flex-1" style={{ color: token.colorText }}>
          {headerLabel}
        </span>

        {ongoingCount > 0 && (
          <Tooltip title={`${ongoingCount} event${ongoingCount > 1 ? 's' : ''} in progress`}>
            <Badge count={ongoingCount} size="small">
              <SyncOutlined style={{ color: '#52c41a', fontSize: 16 }} spin />
            </Badge>
          </Tooltip>
        )}

        <Segmented
          size="small"
          value={view}
          onChange={v => setView(v as ViewMode)}
          options={[
            { label: 'Day',   value: 'day' },
            { label: 'Week',  value: 'week' },
            { label: 'Month', value: 'month' },
          ]}
        />

        <Button type="primary" size="small" icon={<PlusOutlined />} onClick={() => openCreate()}>
          New Event
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
            <CalendarOutlined style={{ color: token.colorPrimary }} />
            {editingEvent ? 'Edit Event' : 'New Event'}
          </div>
        }
        open={showModal}
        onCancel={() => { setShowModal(false); form.resetFields(); setEditingEvent(null); }}
        onOk={handleSave}
        okText={editingEvent ? 'Save Changes' : 'Create Event'}
        cancelText="Cancel"
        confirmLoading={saving}
        width={480}
      >
        <Form form={form} layout="vertical" className="mt-3">
          <Form.Item name="title" rules={[{ required: true, message: 'Please enter a title' }]}>
            <Input
              size="large"
              placeholder="Event title…"
              variant="borderless"
              className="text-lg font-medium px-0"
              style={{ borderBottom: `2px solid ${token.colorPrimary}`, borderRadius: 0 }}
            />
          </Form.Item>

          <Form.Item name="range" label="Date & Time" rules={[{ required: true, message: 'Please select a time range' }]}>
            <RangePicker
              showTime={{ format: 'HH:mm' }}
              format="ddd MM/DD · HH:mm"
              style={{ width: '100%' }}
              placeholder={['Start', 'End']}
            />
          </Form.Item>

          <Form.Item name="all_day" valuePropName="checked">
            <div className="flex items-center gap-2">
              <Switch size="small" />
              <Text className="text-sm">All-day event</Text>
            </div>
          </Form.Item>

          <Form.Item name="location" label="Location">
            <Input
              prefix={<EnvironmentOutlined style={{ color: token.colorTextSecondary }} />}
              placeholder="Location (optional)"
            />
          </Form.Item>

          <Form.Item name="description" label="Description">
            <TextArea rows={2} placeholder="Add a description (optional)" />
          </Form.Item>

          <div className="grid grid-cols-2 gap-3">
            <Form.Item
              name="reminder_min"
              label={
                <span className="flex items-center gap-1.5">
                  <BellOutlined style={{ color: token.colorWarning }} />
                  Reminder before
                </span>
              }
            >
              <InputNumber
                min={1} max={1440}
                placeholder="None"
                addonAfter="min"
                style={{ width: '100%' }}
              />
            </Form.Item>

            <Form.Item
              name="renotify_min"
              label={
                <span className="flex items-center gap-1.5">
                  <ReloadOutlined style={{ color: token.colorPrimary }} />
                  Re-notify every
                </span>
              }
            >
              <InputNumber
                min={1} max={1440}
                placeholder="None"
                addonAfter="min"
                style={{ width: '100%' }}
              />
            </Form.Item>
          </div>
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

function WeekTimeline({ days, today, eventsByDay, token, scrollRef, onAddDay, onSelectEvent }: TimelineProps) {
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
              className="flex-1 flex flex-col items-center py-2 border-l cursor-pointer"
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
                className="text-xl font-bold mt-0.5 w-9 h-9 flex items-center justify-center rounded-full"
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

      <AllDayStrip days={days} eventsByDay={eventsByDay} token={token} onSelectEvent={onSelectEvent} />

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
                  <TimelineEvent key={ev.id} event={ev} token={token} onSelect={onSelectEvent} />
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
  days, eventsByDay, token, onSelectEvent,
}: { days: Dayjs[]; eventsByDay: Record<string, SpaceEvent[]>; token: ReturnType<typeof theme.useToken>['token']; onSelectEvent: (ev: SpaceEvent) => void }) {
  const hasAny = days.some(d => (eventsByDay[d.format('YYYY-MM-DD')] ?? []).some(e => e.all_day));
  if (!hasAny) return null;

  return (
    <div className="flex border-b flex-shrink-0" style={{ borderColor: token.colorBorderSecondary, minHeight: 28 }}>
      <div className="w-14 flex-shrink-0 flex items-center justify-end pr-2">
        <span className="text-xs" style={{ color: token.colorTextQuaternary }}>All day</span>
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

function TimelineEvent({
  event, token, onSelect,
}: { event: SpaceEvent; token: ReturnType<typeof theme.useToken>['token']; onSelect: (ev: SpaceEvent) => void }) {
  const startMin = msToMinutes(event.start_at);
  const endMin = msToMinutes(event.end_at);
  const durationMin = Math.max(endMin - startMin, 30);
  const top = (startMin / 60) * HOUR_H;
  const height = (durationMin / 60) * HOUR_H;
  const color = evColor(event);
  const isOngoing = event.status === 'ongoing';
  const isDone = event.status === 'done' || event.status === 'cancelled';

  return (
    <div
      className="absolute left-1 right-1 rounded-md overflow-hidden cursor-pointer transition-all hover:shadow-md"
      style={{
        top,
        height: Math.max(height, 22),
        background: color + (isDone ? '15' : '20'),
        border: `1px solid ${color}${isDone ? '40' : '50'}`,
        borderLeft: `3px solid ${color}`,
        opacity: isDone ? 0.65 : 1,
        zIndex: 1,
      }}
      onClick={() => onSelect(event)}
    >
      <div className="px-1.5 py-0.5">
        <div className="flex items-center gap-1">
          {isOngoing && <SyncOutlined spin style={{ fontSize: 9, color, flexShrink: 0 }} />}
          <div className="text-xs font-semibold truncate" style={{ color }}>
            {event.title}
          </div>
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
      {/* Bell icon if reminder set */}
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

function MonthGrid({ cursor, weeks, today, eventsByDay, token, onAddDay, onSelectEvent }: MonthGridProps) {
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
                  className="border-l p-1 cursor-pointer relative"
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
                        +{more} more
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
  const isOngoing = event.status === 'ongoing';
  const isDone = event.status === 'done' || event.status === 'cancelled';

  return (
    <Tooltip title={`${event.title}${event.status && event.status !== 'upcoming' ? ` · ${STATUS_CONFIG[event.status]?.label ?? event.status}` : ''}`} mouseEnterDelay={0.6}>
      <div
        className="flex items-center gap-1 rounded px-1 py-0.5 cursor-pointer truncate hover:opacity-80 transition-opacity"
        style={{
          background: event.all_day ? color + '25' : 'transparent',
          borderLeft: `2.5px solid ${color}`,
          opacity: isDone ? 0.6 : 1,
        }}
        onClick={e => { e.stopPropagation(); onSelect(event); }}
      >
        {isOngoing && <SyncOutlined spin style={{ fontSize: 8, color, flexShrink: 0 }} />}
        {!event.all_day && (
          <span className="text-xs flex-shrink-0 font-medium" style={{ color }}>
            {fmt(event.start_at)}
          </span>
        )}
        <span
          className="text-xs truncate"
          style={{
            color: isDone ? token.colorTextSecondary : token.colorText,
            fontWeight: event.all_day ? 600 : 400,
            textDecoration: event.status === 'cancelled' ? 'line-through' : undefined,
          }}
        >
          {event.title}
        </span>
        {event.reminder_min != null && (
          <BellOutlined style={{ fontSize: 9, color: token.colorWarning, flexShrink: 0 }} />
        )}
      </div>
    </Tooltip>
  );
}
