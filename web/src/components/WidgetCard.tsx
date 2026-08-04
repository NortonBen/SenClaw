import { useState, useEffect, useMemo } from 'react';
import { theme } from 'antd';
import dayjs from 'dayjs';
import utc from 'dayjs/plugin/utc';
import timezone from 'dayjs/plugin/timezone';
import {
  ResponsiveContainer,
  BarChart, Bar,
  LineChart, Line,
  AreaChart, Area,
  PieChart, Pie, Cell,
  ScatterChart, Scatter,
  XAxis, YAxis, ZAxis,
  CartesianGrid, Tooltip, Legend,
} from 'recharts';
import type {
  WidgetSpec, ChartData, ChartSeries, ChartPoint, ImageData, ClockData,
  WeatherData, WeatherIcon, VideoData, AudioData, AppWidgetData,
} from '../types';
import { getWidgetCatalog } from '../utils/flowDefaults';

dayjs.extend(utc);
dayjs.extend(timezone);

// Small brand-neutral categorical palette (reused when a series omits `color`).
const PALETTE = ['#5B8FF9', '#61DDAA', '#F6BD16', '#F08BB4', '#7262FD', '#78D3F8', '#FF9D4D', '#269A99'];

// ===== Shared card chrome =====

function CardShell({ title, children }: { title?: string; children: React.ReactNode }) {
  const { token } = theme.useToken();
  return (
    <div
      className="rounded-2xl border p-3 shadow-sm max-w-[80%]"
      style={{ background: token.colorBgContainer, borderColor: token.colorBorderSecondary }}
    >
      {title ? (
        <div
          className="text-[13px] font-semibold mb-2 px-1"
          style={{ color: token.colorText }}
        >
          {title}
        </div>
      ) : null}
      {children}
    </div>
  );
}

function ErrorChip({ label }: { label: string }) {
  const { token } = theme.useToken();
  return (
    <span
      className="inline-flex items-center gap-1.5 rounded-lg px-2.5 py-1 text-xs font-medium"
      style={{ background: token.colorErrorBg, color: token.colorError, border: `1px solid ${token.colorErrorBorder}` }}
    >
      <span aria-hidden>⚠️</span>
      {label}
    </span>
  );
}

// ===== Chart =====

/** Lenient number parse: numbers, "37", "33.5", comma-decimal "33,5". */
function chartAsNum(v: unknown): number | null {
  if (typeof v === 'number' && Number.isFinite(v)) return v;
  if (typeof v === 'string') {
    const s = v.trim();
    if (!s) return null;
    const direct = Number(s);
    if (Number.isFinite(direct)) return direct;
    if (s.includes(',') && !s.includes('.')) {
      const n = Number(s.replace(',', '.'));
      if (Number.isFinite(n)) return n;
    }
  }
  return null;
}

function normalizeChartPoints(raw: unknown): ChartPoint[] {
  if (!Array.isArray(raw)) return [];
  const out: ChartPoint[] = [];
  raw.forEach((p, i) => {
    if (Array.isArray(p)) {
      const y = p.length >= 2 ? chartAsNum(p[1]) : null;
      if (y != null) out.push({ x: p[0] as string | number, y });
    } else if (p && typeof p === 'object') {
      const o = p as { x?: unknown; y?: unknown };
      const y = chartAsNum(o.y);
      if (y != null) out.push({ x: (o.x ?? i) as string | number, y });
    } else {
      const y = chartAsNum(p);
      if (y != null) out.push({ x: i, y });
    }
  });
  return out;
}

const X_KEY_HINTS = ['x', 'date', 'day', 'time', 'label', 'name', 'ngay', 'ngày'];

/**
 * Accepts the same shapes the daemon normalizer takes, so a fenced ```chart
 * block written inline in the reply (which bypasses the daemon) renders
 * identically to a tool-emitted chart: canonical `series` (points as {x,y},
 * [x,y] pairs or bare numbers), `rows` (one object per x — every numeric
 * column becomes a series), or `labels` + `values`.
 */
export function deriveChartSeries(data: ChartData): ChartSeries[] {
  if (Array.isArray(data.series)) {
    return data.series.map((s, i) => ({
      name: s?.name ?? `series ${i + 1}`,
      color: s?.color,
      points: normalizeChartPoints(s?.points),
    }));
  }
  if (Array.isArray(data.rows)) {
    const maps = data.rows.filter(
      (r): r is Record<string, unknown> => !!r && typeof r === 'object' && !Array.isArray(r),
    );
    if (!maps.length) return [];
    let xKey = typeof data.x === 'string' && data.x in maps[0] ? data.x : undefined;
    if (!xKey) xKey = X_KEY_HINTS.find((k) => k in maps[0]);
    if (!xKey) {
      xKey = Object.keys(maps[0]).find(
        (k) => typeof maps[0][k] === 'string' && chartAsNum(maps[0][k]) == null,
      );
    }
    const keys: string[] = [];
    for (const r of maps) {
      for (const k of Object.keys(r)) {
        if (k !== xKey && !keys.includes(k) && chartAsNum(r[k]) != null) keys.push(k);
      }
    }
    return keys.map((key) => ({
      name: key,
      points: maps.flatMap((r, i) => {
        const y = chartAsNum(r[key]);
        if (y == null) return [];
        return [{ x: (xKey ? (r[xKey] as string | number) : i), y }];
      }),
    }));
  }
  if (Array.isArray(data.labels) && Array.isArray(data.values)) {
    const points: ChartPoint[] = [];
    data.labels.forEach((l, i) => {
      const y = chartAsNum(data.values?.[i]);
      if (y != null) points.push({ x: l, y });
    });
    return [{ name: typeof data.name === 'string' ? data.name : 'values', points }];
  }
  return [];
}

function ChartWidget({ data }: { data: ChartData }) {
  const { token } = theme.useToken();
  const series = useMemo(() => deriveChartSeries(data), [data]);
  const colorFor = (s: ChartSeries, i: number) => s.color || PALETTE[i % PALETTE.length];

  // Merge every series' points into row objects keyed by x so recharts can
  // render multiple series (one column per series) against a shared axis.
  const rows = useMemo(() => {
    const byX = new Map<string | number, Record<string, string | number>>();
    for (const s of series) {
      for (const p of s.points ?? []) {
        const key = p.x;
        const row = byX.get(key) ?? { x: key };
        row[s.name] = p.y;
        byX.set(key, row);
      }
    }
    return Array.from(byX.values());
  }, [series]);

  if (series.length === 0) return <ErrorChip label="Chart has no series" />;

  const axisStyle = { fontSize: 11, fill: token.colorTextSecondary };
  const gridColor = token.colorBorderSecondary;

  const tooltipStyle = {
    contentStyle: {
      background: token.colorBgElevated,
      border: `1px solid ${token.colorBorderSecondary}`,
      borderRadius: 8,
      color: token.colorText,
      fontSize: 12,
    },
    labelStyle: { color: token.colorText },
    itemStyle: { color: token.colorText },
  };

  const xAxis = (
    <XAxis
      dataKey="x" tick={axisStyle} stroke={gridColor}
      label={data.xLabel ? { value: data.xLabel, position: 'insideBottom', offset: -2, fontSize: 11, fill: token.colorTextSecondary } : undefined}
    />
  );
  const yAxis = (
    <YAxis
      tick={axisStyle} stroke={gridColor}
      label={data.yLabel ? { value: data.yLabel, angle: -90, position: 'insideLeft', fontSize: 11, fill: token.colorTextSecondary } : undefined}
    />
  );

  let chart: React.ReactElement;
  // Absent chartType → bar, matching the daemon normalizer and desktop.
  switch (data.chartType ?? 'bar') {
    case 'bar':
      chart = (
        <BarChart data={rows} margin={{ top: 8, right: 12, bottom: data.xLabel ? 18 : 4, left: data.yLabel ? 12 : 0 }}>
          <CartesianGrid strokeDasharray="3 3" stroke={gridColor} />
          {xAxis}{yAxis}
          <Tooltip {...tooltipStyle} cursor={{ fill: token.colorFillSecondary }} />
          <Legend wrapperStyle={{ fontSize: 12, color: token.colorText }} />
          {series.map((s, i) => (
            <Bar key={s.name} dataKey={s.name} fill={colorFor(s, i)} radius={[4, 4, 0, 0]} stackId={data.stacked ? 'stack' : undefined} />
          ))}
        </BarChart>
      );
      break;
    case 'line':
      chart = (
        <LineChart data={rows} margin={{ top: 8, right: 12, bottom: data.xLabel ? 18 : 4, left: data.yLabel ? 12 : 0 }}>
          <CartesianGrid strokeDasharray="3 3" stroke={gridColor} />
          {xAxis}{yAxis}
          <Tooltip {...tooltipStyle} />
          <Legend wrapperStyle={{ fontSize: 12, color: token.colorText }} />
          {series.map((s, i) => (
            <Line key={s.name} type="monotone" dataKey={s.name} stroke={colorFor(s, i)} strokeWidth={2} dot={false} />
          ))}
        </LineChart>
      );
      break;
    case 'area':
      chart = (
        <AreaChart data={rows} margin={{ top: 8, right: 12, bottom: data.xLabel ? 18 : 4, left: data.yLabel ? 12 : 0 }}>
          <CartesianGrid strokeDasharray="3 3" stroke={gridColor} />
          {xAxis}{yAxis}
          <Tooltip {...tooltipStyle} />
          <Legend wrapperStyle={{ fontSize: 12, color: token.colorText }} />
          {series.map((s, i) => (
            <Area key={s.name} type="monotone" dataKey={s.name} stroke={colorFor(s, i)} fill={colorFor(s, i)} fillOpacity={0.25} strokeWidth={2} stackId={data.stacked ? 'stack' : undefined} />
          ))}
        </AreaChart>
      );
      break;
    case 'pie': {
      const slices = (series[0]?.points ?? []).map((p) => ({ name: String(p.x), value: p.y }));
      chart = (
        <PieChart margin={{ top: 4, right: 4, bottom: 4, left: 4 }}>
          <Tooltip {...tooltipStyle} />
          <Legend wrapperStyle={{ fontSize: 12, color: token.colorText }} />
          <Pie data={slices} dataKey="value" nameKey="name" cx="50%" cy="50%" outerRadius="75%" label={{ fontSize: 11, fill: token.colorText }}>
            {slices.map((_, i) => <Cell key={i} fill={series[0]?.color && i === 0 ? series[0].color : PALETTE[i % PALETTE.length]} />)}
          </Pie>
        </PieChart>
      );
      break;
    }
    case 'scatter':
      chart = (
        <ScatterChart margin={{ top: 8, right: 12, bottom: data.xLabel ? 18 : 4, left: data.yLabel ? 12 : 0 }}>
          <CartesianGrid strokeDasharray="3 3" stroke={gridColor} />
          <XAxis type="number" dataKey="x" tick={axisStyle} stroke={gridColor}
            label={data.xLabel ? { value: data.xLabel, position: 'insideBottom', offset: -2, fontSize: 11, fill: token.colorTextSecondary } : undefined} />
          <YAxis type="number" dataKey="y" tick={axisStyle} stroke={gridColor}
            label={data.yLabel ? { value: data.yLabel, angle: -90, position: 'insideLeft', fontSize: 11, fill: token.colorTextSecondary } : undefined} />
          <ZAxis range={[50, 50]} />
          <Tooltip {...tooltipStyle} cursor={{ strokeDasharray: '3 3' }} />
          <Legend wrapperStyle={{ fontSize: 12, color: token.colorText }} />
          {series.map((s, i) => (
            <Scatter key={s.name} name={s.name} data={s.points ?? []} fill={colorFor(s, i)} />
          ))}
        </ScatterChart>
      );
      break;
    default:
      return <ErrorChip label={`Unknown chart type: ${String((data as ChartData).chartType)}`} />;
  }

  return (
    <div style={{ width: '100%', height: 260 }}>
      <ResponsiveContainer width="100%" height="100%">
        {chart}
      </ResponsiveContainer>
    </div>
  );
}

// ===== Image =====

function ImageWidget({ data }: { data: ImageData }) {
  const { token } = theme.useToken();
  const src = data.dataUrl || data.url;
  if (!src) return <ErrorChip label="Image has no url/dataUrl" />;
  return (
    <figure className="m-0">
      <a href={src} target="_blank" rel="noopener noreferrer">
        <img
          src={src}
          alt={data.alt ?? data.caption ?? ''}
          className="rounded-xl max-w-full h-auto object-contain cursor-zoom-in"
          style={{ maxHeight: 360, border: `1px solid ${token.colorBorderSecondary}` }}
        />
      </a>
      {data.caption ? (
        <figcaption className="text-[11px] mt-1.5 px-1" style={{ color: token.colorTextSecondary }}>
          {data.caption}
        </figcaption>
      ) : null}
    </figure>
  );
}

// ===== Video =====

/** Guess a MIME from the URL extension so <source type> stays useful when the
 *  emitter omitted `mime`. Query string / fragment are stripped first. */
function guessMime(url: string): string | undefined {
  const ext = url.split(/[?#]/)[0].split('.').pop()?.toLowerCase();
  switch (ext) {
    case 'mp4': case 'm4v': return 'video/mp4';
    case 'webm': return 'video/webm';
    case 'ogv':  return 'video/ogg';
    case 'mov':  return 'video/quicktime';
    default:     return undefined;
  }
}

function VideoWidget({ data }: { data: VideoData }) {
  const { token } = theme.useToken();
  const [failed, setFailed] = useState(false);
  const url = typeof data.url === 'string' ? data.url.trim() : '';

  if (!url) return <ErrorChip label="Video has no url" />;

  // Codec/container the browser can't decode (e.g. some .mov) — don't leave a
  // black box, hand the user a way out to the system player.
  if (failed) {
    return (
      <div className="flex flex-col gap-2 items-start">
        <ErrorChip label="Không phát được video ở đây" />
        <a
          href={url} target="_blank" rel="noopener noreferrer"
          className="text-[12px] underline"
          style={{ color: token.colorLink }}
        >
          Mở video trong tab mới ↗
        </a>
      </div>
    );
  }

  return (
    <figure className="m-0">
      {/* preload=metadata: a chat can accumulate many of these — fetch the
          poster frame and duration, not the whole file, until the user plays. */}
      <video
        key={url}
        src={url}
        poster={data.poster || undefined}
        controls
        playsInline
        autoPlay={data.autoplay === true}
        muted={data.autoplay === true}
        preload="metadata"
        onError={() => setFailed(true)}
        className="rounded-xl block"
        style={{
          maxHeight: 420,
          maxWidth: '100%',
          background: '#000',
          border: `1px solid ${token.colorBorderSecondary}`,
        }}
      >
        <source src={url} type={data.mime || guessMime(url)} />
      </video>
      <figcaption
        className="text-[11px] mt-1.5 px-1 flex items-center gap-2"
        style={{ color: token.colorTextSecondary }}
      >
        {data.caption ? <span className="truncate">{data.caption}</span> : null}
        <a
          href={url} target="_blank" rel="noopener noreferrer"
          className="underline shrink-0 ml-auto"
          style={{ color: token.colorTextTertiary }}
        >
          Mở ngoài ↗
        </a>
      </figcaption>
    </figure>
  );
}

// ===== Audio =====

function guessAudioMime(url: string): string | undefined {
  const ext = url.split(/[?#]/)[0].split('.').pop()?.toLowerCase();
  switch (ext) {
    case 'mp3': return 'audio/mpeg';
    case 'm4a': case 'aac': return 'audio/mp4';
    case 'wav': return 'audio/wav';
    case 'ogg': case 'oga': return 'audio/ogg';
    case 'flac': return 'audio/flac';
    default: return undefined;
  }
}

function AudioWidget({ data }: { data: AudioData }) {
  const { token } = theme.useToken();
  const [failed, setFailed] = useState(false);
  const url = typeof data.url === 'string' ? data.url.trim() : '';

  if (!url) return <ErrorChip label="Audio has no url" />;
  if (failed) {
    return (
      <div className="flex flex-col gap-2 items-start">
        <ErrorChip label="Không phát được audio ở đây" />
        <a
          href={url} target="_blank" rel="noopener noreferrer"
          className="text-[12px] underline" style={{ color: token.colorLink }}
        >
          Mở trong tab mới ↗
        </a>
      </div>
    );
  }
  return (
    <figure className="m-0">
      <audio
        key={url}
        controls
        preload="metadata"
        onError={() => setFailed(true)}
        className="block w-full"
        style={{ minWidth: 260 }}
      >
        <source src={url} type={data.mime || guessAudioMime(url)} />
      </audio>
      {data.caption ? (
        <figcaption className="text-[11px] mt-1.5 px-1" style={{ color: token.colorTextSecondary }}>
          {data.caption}
        </figcaption>
      ) : null}
    </figure>
  );
}

// ===== App widget (Space App / plugin, iframe) =====

const APP_WIDGET_HEIGHT: Record<string, number> = {
  small: 180,
  medium: 320,
  large: 480,
  tall: 560,
};

/**
 * Kind `app`: sandboxed iframe on the entry the daemon resolved at emit time.
 * Fence-emitted specs carry no `entry` — resolve it from `GET /api/widgets`
 * by full id, appending `params` as a query string (params never change the
 * path; the entry path is fixed by the app's manifest).
 */
function AppWidgetFrame({ data }: { data: AppWidgetData }) {
  const { token } = theme.useToken();
  const [entry, setEntry] = useState<string | null | undefined>(data.entry || undefined);
  const [reloadKey, setReloadKey] = useState(0);

  useEffect(() => {
    if (data.entry) { setEntry(data.entry); return; }
    let alive = true;
    getWidgetCatalog().then((catalog) => {
      if (!alive) return;
      const def = catalog.find((w) => w.id === data.id && w.enabled);
      if (!def?.entry) { setEntry(null); return; }
      let url = def.entry;
      const params = data.params ?? {};
      const qs = Object.entries(params)
        .map(([k, v]) => `${encodeURIComponent(k)}=${encodeURIComponent(typeof v === 'string' ? v : JSON.stringify(v))}`)
        .join('&');
      if (qs) url += (url.includes('?') ? '&' : '?') + qs;
      setEntry(url);
    });
    return () => { alive = false; };
  }, [data.entry, data.id, data.params]);

  // refreshMs: reload the iframe on the declared cadence (proxy path has no
  // WebSocket tunnel, so polling is the live-data mechanism there).
  useEffect(() => {
    if (!data.refreshMs || data.refreshMs < 1000) return;
    const id = setInterval(() => setReloadKey((k) => k + 1), data.refreshMs);
    return () => clearInterval(id);
  }, [data.refreshMs]);

  if (entry === null) {
    // Unknown/disabled widget (or catalog unreachable): degrade to the text
    // fallback + a deep link into the app, never a broken frame.
    return (
      <div className="flex flex-col gap-2 items-start px-1 py-1">
        <span className="text-[13px]" style={{ color: token.colorText }}>
          {data.textFallback || `Widget ${data.id} không khả dụng`}
        </span>
        <a
          href={`/space/app/${encodeURIComponent(data.app)}`}
          className="text-[12px] underline" style={{ color: token.colorLink }}
        >
          Mở app {data.app} ↗
        </a>
      </div>
    );
  }
  if (entry === undefined) {
    return <div className="text-[12px] px-1 py-2" style={{ color: token.colorTextTertiary }}>Đang tải widget…</div>;
  }
  const height = APP_WIDGET_HEIGHT[data.size ?? 'medium'] ?? APP_WIDGET_HEIGHT.medium;
  return (
    <div>
      <iframe
        key={`${entry}#${reloadKey}`}
        src={entry}
        title={data.id}
        // Same sandbox as SpaceAppFrame; app widgets share the app's trust level.
        sandbox="allow-forms allow-modals allow-popups allow-same-origin allow-scripts"
        className="rounded-xl block w-full"
        style={{ height, border: `1px solid ${token.colorBorderSecondary}`, background: token.colorBgLayout }}
      />
      <div className="text-[11px] mt-1 px-1 flex items-center" style={{ color: token.colorTextTertiary }}>
        <span className="truncate">{data.id}</span>
        <a
          href={`/space/app/${encodeURIComponent(data.app)}`}
          className="underline shrink-0 ml-auto" style={{ color: token.colorTextTertiary }}
        >
          Mở app ↗
        </a>
      </div>
    </div>
  );
}

// ===== Clock =====

function ClockWidget({ data }: { data: ClockData }) {
  const { token } = theme.useToken();
  const [now, setNow] = useState(() => dayjs());

  useEffect(() => {
    const id = setInterval(() => setNow(dayjs()), 1000);
    return () => clearInterval(id);
  }, []);

  const zoned = useMemo(() => {
    try {
      return data.tz ? now.tz(data.tz) : now;
    } catch {
      return now; // invalid tz → local
    }
  }, [now, data.tz]);

  const showSeconds = data.showSeconds !== false;
  const timeFmt = (data.format24h !== false ? 'HH:mm' : 'hh:mm') + (showSeconds ? ':ss' : '') + (data.format24h !== false ? '' : ' A');
  const timeStr = zoned.format(timeFmt);
  const dateStr = data.showDate !== false ? zoned.format('dddd, D MMM YYYY') : null;

  return (
    <div className="flex flex-col items-center py-3 px-2">
      {data.label ? (
        <div className="text-[12px] font-medium mb-1" style={{ color: token.colorTextSecondary }}>{data.label}</div>
      ) : null}
      <div className="font-mono tabular-nums tracking-tight" style={{ fontSize: 40, lineHeight: 1.1, color: token.colorText }}>
        {timeStr}
      </div>
      {dateStr ? (
        <div className="text-[12px] mt-1" style={{ color: token.colorTextSecondary }}>{dateStr}</div>
      ) : null}
      {data.tz ? (
        <div className="text-[10px] mt-1 uppercase tracking-wide" style={{ color: token.colorTextTertiary }}>{data.tz}</div>
      ) : null}
    </div>
  );
}

// ===== Weather =====

const WEATHER_GLYPH: Record<WeatherIcon, string> = {
  sunny: '☀️',
  partly_cloudy: '⛅',
  cloudy: '☁️',
  rain: '🌧️',
  thunderstorm: '⛈️',
  snow: '❄️',
  fog: '🌫️',
  wind: '💨',
};

function glyph(icon: string | undefined): string {
  return (icon && WEATHER_GLYPH[icon as WeatherIcon]) || '🌡️';
}

function WeatherWidget({ data }: { data: WeatherData }) {
  const { token } = theme.useToken();
  if (!data.current) return <ErrorChip label="Weather has no current conditions" />;
  const unit = data.unit === 'F' ? '°F' : '°C';
  const daily = (data.daily ?? []).slice(0, 7);

  return (
    <div>
      {/* Current */}
      <div className="flex items-center gap-3 px-1">
        <span style={{ fontSize: 44, lineHeight: 1 }}>{glyph(data.current.icon)}</span>
        <div className="flex-1">
          <div className="flex items-baseline gap-2">
            <span className="font-semibold" style={{ fontSize: 32, color: token.colorText }}>{data.current.temp}{unit}</span>
            <span className="text-[13px]" style={{ color: token.colorTextSecondary }}>{data.current.condition}</span>
          </div>
          <div className="text-[12px]" style={{ color: token.colorTextSecondary }}>{data.location}</div>
          <div className="flex gap-3 text-[11px] mt-0.5" style={{ color: token.colorTextTertiary }}>
            {data.current.humidity != null ? <span>💧 {data.current.humidity}%</span> : null}
            {data.current.wind != null ? <span>💨 {data.current.wind} km/h</span> : null}
          </div>
        </div>
      </div>

      {/* 7-day */}
      {daily.length > 0 ? (
        <div className="flex gap-1 mt-3 overflow-x-auto">
          {daily.map((d, i) => (
            <div
              key={i}
              className="flex flex-col items-center gap-1 px-2 py-2 rounded-xl flex-1 min-w-[52px]"
              style={{ background: token.colorFillQuaternary }}
            >
              <span className="text-[11px] font-medium" style={{ color: token.colorTextSecondary }}>{d.day}</span>
              <span style={{ fontSize: 20 }}>{glyph(d.icon)}</span>
              <span className="text-[11px]" style={{ color: token.colorText }}>
                <span className="font-semibold">{d.hi}°</span>{' '}
                <span style={{ color: token.colorTextTertiary }}>{d.lo}°</span>
              </span>
            </div>
          ))}
        </div>
      ) : null}
    </div>
  );
}

// ===== Dispatcher =====

export function WidgetCard({ widget }: { widget: WidgetSpec }) {
  if (!widget || typeof widget !== 'object' || !widget.kind) {
    return <CardShell><ErrorChip label="Malformed widget" /></CardShell>;
  }
  const data = (widget.data ?? {}) as never;

  let body: React.ReactNode;
  switch (widget.kind) {
    case 'chart':  body = <ChartWidget data={data} />; break;
    case 'image':  body = <ImageWidget data={data} />; break;
    case 'clock':  body = <ClockWidget data={data} />; break;
    case 'weather': body = <WeatherWidget data={data} />; break;
    case 'video':  body = <VideoWidget data={data} />; break;
    case 'audio':  body = <AudioWidget data={data} />; break;
    case 'app':    body = <AppWidgetFrame data={data} />; break;
    default:       body = <ErrorChip label={`Unknown widget kind: ${String(widget.kind)}`} />;
  }

  return <CardShell title={widget.title}>{body}</CardShell>;
}

export default WidgetCard;
