import { useEffect, useState } from 'react';
import { Empty, Input, Segmented, Spin, Typography, theme, App as AntApp } from 'antd';
import { api, type CallLink, type Exploration } from '../api';
import { kindColor, truncate } from '../lib';

const { Text } = Typography;
const { Search } = Input;

interface Props {
  reloadKey: number;
  indexed: boolean;
  focus?: string;
  onFocusChange: (name: string) => void;
}

const NODE_W = 158;
const NODE_H = 30;
const GAP_Y = 46;
const W = 1040;
const CALLER_X = 120;
const CENTER_X = W / 2;
const CALLEE_X = W - 120;
const CAP = 14;

interface Node {
  name: string;
  path: string;
  line: number;
  kind: string;
  x: number;
  y: number;
  external?: boolean;
  clickable?: boolean;
}

export function GraphView({ reloadKey, indexed, focus, onFocusChange }: Props) {
  const { token } = theme.useToken();
  const { message } = AntApp.useApp();
  const [name, setName] = useState(focus ?? '');
  const [side, setSide] = useState<'both' | 'callers' | 'callees'>('both');
  const [data, setData] = useState<Exploration | null>(null);
  const [loading, setLoading] = useState(false);

  useEffect(() => { if (focus) setName(focus); }, [focus]);

  useEffect(() => {
    if (!name.trim()) { setData(null); return; }
    let alive = true;
    setLoading(true);
    api.explore(name.trim(), 3)
      .then((d) => { if (alive) setData(d); })
      .catch((e) => message.error((e as Error).message))
      .finally(() => { if (alive) setLoading(false); });
    return () => { alive = false; };
  }, [name, reloadKey, message]);

  const pick = (n: string) => { setName(n); onFocusChange(n); };

  const center = data?.matches?.[0];
  const callers = side !== 'callees' ? (data?.callers ?? []).slice(0, CAP) : [];
  const callees = side !== 'callers' ? (data?.callees ?? []).slice(0, CAP) : [];
  const rows = Math.max(callers.length, callees.length, 1);
  const H = Math.max(rows * GAP_Y + 90, 260);
  const cy = H / 2;

  const colY = (i: number, n: number) => cy - ((n - 1) * GAP_Y) / 2 + i * GAP_Y;

  const callerNodes: Node[] = callers.map((c, i) => mkNode(c, CALLER_X, colY(i, callers.length), true));
  const calleeNodes: Node[] = callees.map((c, i) => mkNode(c, CALLEE_X, colY(i, callees.length), c.path !== '<external>' && c.kind !== 'external'));

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%', minHeight: 0 }}>
      <div style={{ padding: 12, borderBottom: `1px solid ${token.colorBorderSecondary}`, display: 'flex', gap: 12, alignItems: 'center', flexWrap: 'wrap' }}>
        <Search
          key={name}
          placeholder="Symbol để vẽ call graph…"
          enterButton
          defaultValue={name}
          loading={loading}
          onSearch={(v) => pick(v.trim())}
          disabled={!indexed}
          style={{ maxWidth: 360 }}
        />
        <Segmented
          value={side}
          onChange={(v) => setSide(v as typeof side)}
          options={[
            { label: 'Cả hai', value: 'both' },
            { label: 'Callers', value: 'callers' },
            { label: 'Callees', value: 'callees' },
          ]}
        />
        {center ? (
          <Text type="secondary" style={{ fontSize: 12 }}>
            {data?.callers.length ?? 0} callers · {data?.callees.length ?? 0} callees
            {(data?.callers.length ?? 0) > CAP || (data?.callees.length ?? 0) > CAP ? ' (hiển thị tối đa ' + CAP + '/bên)' : ''}
          </Text>
        ) : null}
      </div>

      <div style={{ flex: 1, minHeight: 0, overflow: 'auto', padding: 16 }}>
        {loading ? (
          <div style={{ textAlign: 'center', padding: 60 }}><Spin /></div>
        ) : !name ? (
          <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description={<Text type="secondary">Nhập một symbol để vẽ đồ thị logic gọi hàm.</Text>} />
        ) : !center ? (
          <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description={<Text type="secondary">Không tìm thấy “{name}”. Thử tên khác.</Text>} />
        ) : (
          <svg viewBox={`0 0 ${W} ${H}`} width="100%" style={{ minHeight: H, display: 'block' }} preserveAspectRatio="xMidYMid meet">
            <defs>
              <marker id="arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
                <path d="M0,0 L10,5 L0,10 z" fill={token.colorTextTertiary} />
              </marker>
            </defs>

            {/* edges caller -> center */}
            {callerNodes.map((n) => (
              <path key={'ec' + n.name + n.y} d={edge(n.x + NODE_W / 2, n.y, CENTER_X - NODE_W / 2 - 6, cy)}
                fill="none" stroke={token.colorTextQuaternary} strokeWidth={1.5} markerEnd="url(#arrow)" />
            ))}
            {/* edges center -> callee */}
            {calleeNodes.map((n) => (
              <path key={'ee' + n.name + n.y} d={edge(CENTER_X + NODE_W / 2, cy, n.x - NODE_W / 2 - 6, n.y)}
                fill="none" stroke={token.colorTextQuaternary} strokeWidth={1.5} markerEnd="url(#arrow)" />
            ))}

            {/* nodes */}
            {callerNodes.map((n) => <GNode key={'nc' + n.name + n.y} n={n} token={token} onPick={pick} />)}
            {calleeNodes.map((n) => <GNode key={'ne' + n.name + n.y} n={n} token={token} onPick={pick} />)}
            {center && (
              <GNode
                n={{ name: center.name, path: center.path, line: center.start_line, kind: center.kind, x: CENTER_X, y: cy }}
                token={token}
                center
                onPick={pick}
              />
            )}
          </svg>
        )}
      </div>
    </div>
  );

  function mkNode(c: CallLink, x: number, y: number, clickable: boolean): Node {
    const external = c.path === '<external>' || c.kind === 'external';
    return { name: c.name, path: c.path, line: c.start_line, kind: c.kind, x, y, external, clickable: clickable && !external };
  }
}

/** Cubic bezier between two points, bowed horizontally. */
function edge(x1: number, y1: number, x2: number, y2: number): string {
  const mx = (x1 + x2) / 2;
  return `M${x1},${y1} C${mx},${y1} ${mx},${y2} ${x2},${y2}`;
}

function GNode({
  n, token, center, onPick,
}: {
  n: Node;
  token: ReturnType<typeof theme.useToken>['token'];
  center?: boolean;
  onPick: (name: string) => void;
}) {
  const w = center ? NODE_W + 16 : NODE_W;
  const accent = n.external ? token.colorTextTertiary : kindColor(n.kind);
  const clickable = center ? true : n.clickable;
  return (
    <g
      transform={`translate(${n.x - w / 2}, ${n.y - NODE_H / 2})`}
      style={{ cursor: clickable ? 'pointer' : 'default' }}
      onClick={() => clickable && onPick(n.name)}
    >
      <title>{n.external ? `${n.name} (external)` : `${n.name} — ${n.path}:${n.line}`}</title>
      <rect
        width={w} height={NODE_H} rx={7}
        fill={center ? token.colorPrimaryBg : token.colorFillTertiary}
        stroke={center ? token.colorPrimary : token.colorBorderSecondary}
        strokeWidth={center ? 2 : 1}
      />
      <rect width={4} height={NODE_H} rx={2} fill={accent} />
      <text
        x={w / 2} y={NODE_H / 2 + 4} textAnchor="middle"
        fontSize={12.5} fontWeight={center ? 700 : 500}
        fontFamily="ui-monospace, SFMono-Regular, Menlo, monospace"
        fill={n.external ? token.colorTextTertiary : token.colorText}
      >
        {truncate(n.name, center ? 20 : 17)}
      </text>
    </g>
  );
}
