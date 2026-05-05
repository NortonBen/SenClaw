import React, { useEffect, useRef, useState, useMemo, useCallback } from 'react';
import {
  Button, Modal, Form, Input, DatePicker, InputNumber, Switch,
  Typography, Spin, Popconfirm, Tooltip, theme, Segmented,
} from 'antd';
import {
  PlusOutlined, DeleteOutlined, BellOutlined, LeftOutlined, RightOutlined,
  EnvironmentOutlined, ClockCircleOutlined,
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

const HOUR_H = 56;          // px per hour in timeline
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

type ViewMode = 'week' | 'month' | 'day';

const { RangePicker } = DatePicker;
const { TextArea } = Input;
const { Text } = Typography;

// ─── Utils ────────────────────────────────────────────────────────────────────

function evColor(ev: SpaceEvent) {
  return ev.color ?? SOURCE_COLOR[ev.source] ?? '#5BBFE8';
}

function fmt(ms: number) {
  return dayjs(ms).format('HH:mm');
}

function msToMinutes(ms: number) {
  const d = dayjs(ms);
  return d.hour() * 60 + d.minute();
}

// ─── Props ────────────────────────────────────────────────────────────────────

interface Props { hook: UseSpaceHook }

// ─── Main component ───────────────────────────────────────────────────────────

export function CalendarView({ hook }: Props) {
  const { token } = theme.useToken();
  const [view, setView] = useState<ViewMode>('week');
  const [cursor, setCursor] = useState<Dayjs>(dayjs());
  const [showModal, setShowModal] = useState(false);
  const [modalDefault, setModalDefault] = useState<Dayjs | null>(null);
  const [form] = Form.useForm();
  const [saving, setSaving] = useState(false);
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
    // month
    const mStart = cursor.startOf('month');
    const mEnd = cursor.endOf('month');
    // pad to full weeks
    const gridStart = mStart.startOf('week');
    const gridEnd = mEnd.endOf('week');
    const total = gridEnd.diff(gridStart, 'day') + 1;
    const cells = Array.from({ length: total }, (_, i) => gridStart.add(i, 'day'));
    const weeks: Dayjs[][] = [];
    for (let i = 0; i < cells.length; i += 7) weeks.push(cells.slice(i, i + 7));
    return {
      rangeStart: gridStart,
      rangeEnd: gridEnd,
      weekDays: [],
      monthGrid: weeks,
    };
  }, [view, cursor.valueOf()]);

  useEffect(() => {
    hook.loadEvents(rangeStart.valueOf(), rangeEnd.valueOf());
  }, [rangeStart.valueOf(), rangeEnd.valueOf()]);

  // Scroll to current hour on mount / view change
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

  // ── Create event ───────────────────────────────────────────────────────────

  const openCreate = (defaultDay?: Dayjs) => {
    setModalDefault(defaultDay ?? cursor);
    form.resetFields();
    if (defaultDay) {
      form.setFieldValue('range', [
        defaultDay.hour(9).minute(0),
        defaultDay.hour(10).minute(0),
      ]);
    }
    setShowModal(true);
  };

  const handleCreate = async () => {
    try {
      const vals = await form.validateFields();
      setSaving(true);
      const [start, end]: [Dayjs, Dayjs] = vals.range;
      await hook.createEvent({
        title: vals.title,
        description: vals.description ?? null,
        start_at: start.valueOf(),
        end_at: end.valueOf(),
        all_day: vals.all_day ?? false,
        location: vals.location ?? null,
        color: null,
        reminder_min: vals.reminder_min ?? null,
      });
      setShowModal(false);
      form.resetFields();
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
        {/* Nav */}
        <div className="flex items-center gap-1">
          <Button
            size="small" type="text" shape="circle"
            icon={<LeftOutlined />}
            onClick={() => nav(-1)}
          />
          <Button
            size="small" type="default"
            onClick={() => setCursor(dayjs())}
            style={{ minWidth: 64, fontSize: 12 }}
          >
            Hôm nay
          </Button>
          <Button
            size="small" type="text" shape="circle"
            icon={<RightOutlined />}
            onClick={() => nav(1)}
          />
        </div>

        {/* Label */}
        <span className="font-semibold text-sm flex-1" style={{ color: token.colorText }}>
          {headerLabel}
        </span>

        {/* View switcher */}
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

        <Button
          type="primary" size="small" icon={<PlusOutlined />}
          onClick={() => openCreate()}
        >
          Thêm sự kiện
        </Button>
      </div>

      {/* ── Loading ──────────────────────────────────────────────────────────── */}
      {hook.eventsLoading && (
        <div className="flex justify-center py-12"><Spin size="large" /></div>
      )}

      {/* ── Day / Week timeline view ─────────────────────────────────────────── */}
      {!hook.eventsLoading && (view === 'week' || view === 'day') && (
        <WeekTimeline
          days={view === 'day' ? [cursor] : weekDays}
          today={today}
          eventsByDay={eventsByDay}
          hook={hook}
          token={token}
          scrollRef={scrollRef}
          onAddDay={openCreate}
        />
      )}

      {/* ── Month grid view ──────────────────────────────────────────────────── */}
      {!hook.eventsLoading && view === 'month' && (
        <MonthGrid
          cursor={cursor}
          weeks={monthGrid}
          today={today}
          eventsByDay={eventsByDay}
          hook={hook}
          token={token}
          onAddDay={openCreate}
        />
      )}

      {/* ── Create modal ─────────────────────────────────────────────────────── */}
      <Modal
        title={
          <div className="flex items-center gap-2">
            <span
              className="w-3 h-3 rounded-full inline-block"
              style={{ background: token.colorPrimary }}
            />
            Tạo sự kiện mới
          </div>
        }
        open={showModal}
        onCancel={() => { setShowModal(false); form.resetFields(); }}
        onOk={handleCreate}
        okText="Tạo sự kiện"
        cancelText="Hủy"
        confirmLoading={saving}
        width={480}
      >
        <Form form={form} layout="vertical" className="mt-3">
          <Form.Item
            name="title"
            rules={[{ required: true, message: 'Nhập tiêu đề sự kiện' }]}
          >
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
            <span className="flex items-center gap-1">
              <BellOutlined /> Nhắc nhở trước
            </span>
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
}

function WeekTimeline({ days, today, eventsByDay, hook, token, scrollRef, onAddDay }: TimelineProps) {
  const nowMin = dayjs().hour() * 60 + dayjs().minute();
  const isTodayVisible = days.some(d => d.format('YYYY-MM-DD') === today);

  const colCount = days.length;

  return (
    <div className="flex flex-col flex-1 min-h-0">
      {/* Day header row */}
      <div
        className="flex border-b flex-shrink-0"
        style={{ borderColor: token.colorBorderSecondary, background: token.colorBgContainer }}
      >
        {/* Gutter */}
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

      {/* All-day strip */}
      <AllDayStrip days={days} eventsByDay={eventsByDay} hook={hook} token={token} />

      {/* Scrollable time grid */}
      <div ref={scrollRef} className="flex-1 overflow-y-auto relative">
        <div className="flex" style={{ minHeight: HOUR_H * 24 }}>
          {/* Hour labels */}
          <div className="w-14 flex-shrink-0 relative">
            {HOURS.map(h => (
              <div
                key={h}
                className="absolute flex items-start justify-end pr-2"
                style={{
                  top: h * HOUR_H - 8,
                  height: HOUR_H,
                  width: '100%',
                }}
              >
                {h > 0 && (
                  <span
                    className="text-xs"
                    style={{ color: token.colorTextQuaternary, lineHeight: 1 }}
                  >
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
              <div
                key={key}
                className="flex-1 relative border-l"
                style={{ borderColor: token.colorBorderSecondary }}
              >
                {/* Hour grid lines */}
                {HOURS.map(h => (
                  <div
                    key={h}
                    className="absolute w-full border-t"
                    style={{
                      top: h * HOUR_H,
                      borderColor: h % 1 === 0
                        ? token.colorBorderSecondary
                        : token.colorFillSecondary,
                      opacity: 0.6,
                    }}
                  />
                ))}

                {/* Today column highlight */}
                {isToday && (
                  <div
                    className="absolute inset-0 pointer-events-none"
                    style={{ background: token.colorPrimary + '06' }}
                  />
                )}

                {/* Events */}
                {dayEvs.map(ev => (
                  <TimelineEvent
                    key={ev.id}
                    event={ev}
                    hook={hook}
                    token={token}
                  />
                ))}

                {/* Now indicator */}
                {isToday && (
                  <div
                    className="absolute left-0 right-0 z-10 flex items-center pointer-events-none"
                    style={{ top: (nowMin / 60) * HOUR_H }}
                  >
                    <div
                      className="w-2.5 h-2.5 rounded-full flex-shrink-0 -ml-1.5"
                      style={{ background: '#f5222d' }}
                    />
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
  days, eventsByDay, hook, token,
}: Pick<TimelineProps, 'days' | 'eventsByDay' | 'hook' | 'token'>) {
  const allDayEvs = days.some(d => (eventsByDay[d.format('YYYY-MM-DD')] ?? []).some(e => e.all_day));
  if (!allDayEvs) return null;

  return (
    <div
      className="flex border-b flex-shrink-0"
      style={{ borderColor: token.colorBorderSecondary, minHeight: 28 }}
    >
      <div
        className="w-14 flex-shrink-0 flex items-center justify-end pr-2"
      >
        <span className="text-xs" style={{ color: token.colorTextQuaternary }}>Cả ngày</span>
      </div>
      {days.map(d => {
        const key = d.format('YYYY-MM-DD');
        const evs = (eventsByDay[key] ?? []).filter(e => e.all_day);
        return (
          <div key={key} className="flex-1 border-l px-0.5 py-0.5 space-y-0.5"
            style={{ borderColor: token.colorBorderSecondary }}>
            {evs.map(ev => (
              <div
                key={ev.id}
                className="rounded px-1.5 py-0.5 text-xs font-medium truncate"
                style={{
                  background: evColor(ev) + '30',
                  borderLeft: `3px solid ${evColor(ev)}`,
                  color: token.colorText,
                }}
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
}

function TimelineEvent({ event, hook, token }: TEProps) {
  const startMin = msToMinutes(event.start_at);
  const endMin = msToMinutes(event.end_at);
  const durationMin = Math.max(endMin - startMin, 30); // min 30 min height
  const top = (startMin / 60) * HOUR_H;
  const height = (durationMin / 60) * HOUR_H;
  const color = evColor(event);

  return (
    <Tooltip
      title={
        <div className="text-xs space-y-0.5">
          <div className="font-semibold">{event.title}</div>
          <div className="flex items-center gap-1">
            <ClockCircleOutlined />
            {fmt(event.start_at)} – {fmt(event.end_at)}
          </div>
          {event.location && (
            <div className="flex items-center gap-1">
              <EnvironmentOutlined /> {event.location}
            </div>
          )}
          {event.reminder_min != null && (
            <div className="flex items-center gap-1">
              <BellOutlined /> Nhắc {event.reminder_min} phút trước
            </div>
          )}
        </div>
      }
      placement="right"
    >
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
        {/* Delete on hover */}
        <Popconfirm
          title="Xóa sự kiện?"
          onConfirm={() => hook.deleteEvent(event.id)}
          okText="Xóa"
          cancelText="Hủy"
        >
          <button
            className="absolute top-0.5 right-0.5 opacity-0 group-hover:opacity-100 transition-opacity rounded p-0.5"
            style={{ background: color + '30', border: 'none', cursor: 'pointer' }}
            onClick={e => e.stopPropagation()}
          >
            <DeleteOutlined style={{ fontSize: 9, color }} />
          </button>
        </Popconfirm>
      </div>
    </Tooltip>
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
}

function MonthGrid({ cursor, weeks, today, eventsByDay, hook, token, onAddDay }: MonthGridProps) {
  const currentMonth = cursor.month();

  return (
    <div className="flex flex-col flex-1 min-h-0">
      {/* Day-of-week header */}
      <div
        className="grid grid-cols-7 border-b flex-shrink-0"
        style={{ borderColor: token.colorBorderSecondary, background: token.colorBgContainer }}
      >
        {DAY_NAMES.map(name => (
          <div
            key={name}
            className="text-center py-2 text-xs font-semibold uppercase tracking-wide"
            style={{ color: token.colorTextSecondary }}
          >
            {name}
          </div>
        ))}
      </div>

      {/* Weeks */}
      <div className="flex-1 overflow-y-auto">
        {weeks.map((week, wi) => (
          <div
            key={wi}
            className="grid grid-cols-7 border-b"
            style={{
              borderColor: token.colorBorderSecondary,
              minHeight: 100,
            }}
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
                    background: isToday
                      ? token.colorPrimary + '08'
                      : !isCurrentMonth
                        ? token.colorFillQuaternary
                        : undefined,
                  }}
                  onClick={() => onAddDay(day)}
                >
                  {/* Day number */}
                  <div className="flex justify-end mb-1">
                    <span
                      className="text-xs font-semibold w-6 h-6 flex items-center justify-center rounded-full"
                      style={{
                        background: isToday ? token.colorPrimary : 'transparent',
                        color: isToday
                          ? '#fff'
                          : !isCurrentMonth
                            ? token.colorTextQuaternary
                            : token.colorText,
                      }}
                    >
                      {day.date()}
                    </span>
                  </div>

                  {/* Events */}
                  <div className="space-y-0.5">
                    {visible.map(ev => (
                      <MonthEventChip key={ev.id} event={ev} hook={hook} token={token} />
                    ))}
                    {more > 0 && (
                      <div
                        className="text-xs px-1"
                        style={{ color: token.colorTextSecondary }}
                      >
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
  event, hook, token,
}: { event: SpaceEvent; hook: UseSpaceHook; token: ReturnType<typeof theme.useToken>['token'] }) {
  const color = evColor(event);
  return (
    <Tooltip
      title={
        <div className="text-xs">
          <div className="font-semibold">{event.title}</div>
          <div>{event.all_day ? 'Cả ngày' : `${fmt(event.start_at)} – ${fmt(event.end_at)}`}</div>
          {event.location && <div>📍 {event.location}</div>}
        </div>
      }
    >
      <Popconfirm
        title="Xóa sự kiện này?"
        onConfirm={e => { e?.stopPropagation(); hook.deleteEvent(event.id); }}
        onCancel={e => e?.stopPropagation()}
        okText="Xóa"
        cancelText="Hủy"
      >
        <div
          className="flex items-center gap-1 rounded px-1 py-0.5 cursor-pointer truncate hover:opacity-80 transition-opacity"
          style={{
            background: event.all_day ? color + '25' : 'transparent',
            borderLeft: `2.5px solid ${color}`,
          }}
          onClick={e => e.stopPropagation()}
        >
          {!event.all_day && (
            <span className="text-xs flex-shrink-0 font-medium" style={{ color }}>
              {fmt(event.start_at)}
            </span>
          )}
          <span
            className="text-xs truncate"
            style={{ color: token.colorText, fontWeight: event.all_day ? 600 : 400 }}
          >
            {event.title}
          </span>
          {event.reminder_min != null && (
            <BellOutlined style={{ fontSize: 9, color: token.colorWarning, flexShrink: 0 }} />
          )}
        </div>
      </Popconfirm>
    </Tooltip>
  );
}
