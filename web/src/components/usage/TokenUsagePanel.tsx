import { useEffect, useState } from 'react';
import { Card, Col, Row, Statistic, Table, Typography } from 'antd';
import { ThunderboltOutlined, DollarOutlined, DatabaseOutlined } from '@ant-design/icons';
import {
  Area,
  AreaChart,
  CartesianGrid,
  Legend,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from 'recharts';

const { Text, Title } = Typography;

// Categorical pair validated for the dark surface (CVD ΔE 19.9, contrast ≥3:1):
// cyan = tokens in (harmonizes with the app accent #5BBFE8), amber = tokens out.
const C_IN = '#3D9AC7';
const C_OUT = '#BA7A35';
const INK = 'rgba(255,255,255,0.85)';
const INK_2 = 'rgba(255,255,255,0.45)';
const GRID = 'rgba(255,255,255,0.08)';
const CARD_STYLE = {
  background: 'rgba(13, 13, 31, 0.4)',
  borderColor: 'rgba(255,255,255,0.05)',
  borderRadius: '12px',
} as const;

interface Totals {
  calls: number;
  inputTokens: number;
  outputTokens: number;
  cacheCreationTokens: number;
  cacheReadTokens: number;
  estCostUsd: number;
  unpricedTokens: number;
}

interface DailyRow extends Totals {
  date: string;
}

interface BreakdownRow extends Totals {
  key: string;
}

const totalIn = (t: Totals) => t.inputTokens + t.cacheCreationTokens + t.cacheReadTokens;

function fmtTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}k`;
  return `${n}`;
}

/** Cost label that never fakes $0: with zero priced volume and some unpriced
 * volume the honest answer is "n/a". */
function fmtCost(t: Totals | undefined): string {
  if (!t) return '—';
  if (t.estCostUsd === 0 && t.unpricedTokens > 0) return 'n/a';
  return `$${t.estCostUsd.toFixed(t.estCostUsd >= 10 ? 2 : 3)}`;
}

export function TokenUsagePanel({ showTitle = true }: { showTitle?: boolean } = {}) {
  const [overview, setOverview] = useState<{ today?: Totals; week?: Totals; month?: Totals }>({});
  const [daily, setDaily] = useState<DailyRow[]>([]);
  const [byModel, setByModel] = useState<BreakdownRow[]>([]);
  const [byApp, setByApp] = useState<BreakdownRow[]>([]);

  useEffect(() => {
    let alive = true;
    const load = async () => {
      try {
        const [ov, dl, bm, ba] = await Promise.all([
          fetch('/api/usage/overview').then(r => r.json()),
          fetch('/api/usage/daily?days=30').then(r => r.json()),
          fetch('/api/usage/breakdown?by=model&days=7').then(r => r.json()),
          fetch('/api/usage/breakdown?by=app&days=7').then(r => r.json()),
        ]);
        if (!alive) return;
        setOverview(ov ?? {});
        setDaily(dl?.rows ?? []);
        setByModel(bm?.rows ?? []);
        setByApp((ba?.rows ?? []).filter((r: BreakdownRow) => r.key !== ''));
      } catch {
        /* daemon without usage endpoints — panel stays empty */
      }
    };
    load();
    const timer = setInterval(load, 30_000);
    return () => {
      alive = false;
      clearInterval(timer);
    };
  }, []);

  const today = overview.today;
  const cacheShare =
    today && totalIn(today) > 0 ? Math.round((today.cacheReadTokens / totalIn(today)) * 100) : 0;

  const chartData = daily.map(d => ({
    date: d.date.slice(5), // MM-DD
    in: totalIn(d),
    out: d.outputTokens,
  }));

  const breakdownColumns = (keyTitle: string) => [
    {
      title: <Text style={{ color: INK_2 }}>{keyTitle}</Text>,
      dataIndex: 'key',
      key: 'key',
      ellipsis: true,
      render: (v: string) => <Text style={{ color: INK }}>{v || '(none)'}</Text>,
    },
    {
      title: <Text style={{ color: INK_2 }}>Calls</Text>,
      dataIndex: 'calls',
      key: 'calls',
      width: 70,
      align: 'right' as const,
      render: (v: number) => <Text style={{ color: INK_2 }}>{v}</Text>,
    },
    {
      title: <Text style={{ color: INK_2 }}>In</Text>,
      key: 'in',
      width: 80,
      align: 'right' as const,
      render: (_: unknown, r: BreakdownRow) => (
        <Text style={{ color: INK }}>{fmtTokens(totalIn(r))}</Text>
      ),
    },
    {
      title: <Text style={{ color: INK_2 }}>Out</Text>,
      dataIndex: 'outputTokens',
      key: 'out',
      width: 80,
      align: 'right' as const,
      render: (v: number) => <Text style={{ color: INK }}>{fmtTokens(v)}</Text>,
    },
    {
      title: <Text style={{ color: INK_2 }}>Cost</Text>,
      key: 'cost',
      width: 90,
      align: 'right' as const,
      render: (_: unknown, r: BreakdownRow) => <Text style={{ color: INK }}>{fmtCost(r)}</Text>,
    },
  ];

  return (
    <div style={{ marginTop: 32 }}>
      {showTitle && (
        <Title level={4} style={{ color: INK, marginBottom: 16 }}>
          Token Usage
        </Title>
      )}
      <Row gutter={[24, 24]}>
        <Col xs={24} sm={12} lg={6}>
          <Card style={CARD_STYLE} bodyStyle={{ padding: '24px' }}>
            <Statistic
              title={<Text style={{ color: INK_2 }}>Tokens in (today)</Text>}
              value={today ? fmtTokens(totalIn(today)) : '—'}
              prefix={<ThunderboltOutlined style={{ color: C_IN, marginRight: 8 }} />}
              valueStyle={{ color: INK, fontSize: 28 }}
            />
          </Card>
        </Col>
        <Col xs={24} sm={12} lg={6}>
          <Card style={CARD_STYLE} bodyStyle={{ padding: '24px' }}>
            <Statistic
              title={<Text style={{ color: INK_2 }}>Tokens out (today)</Text>}
              value={today ? fmtTokens(today.outputTokens) : '—'}
              prefix={<ThunderboltOutlined style={{ color: C_OUT, marginRight: 8 }} />}
              valueStyle={{ color: INK, fontSize: 28 }}
            />
          </Card>
        </Col>
        <Col xs={24} sm={12} lg={6}>
          <Card style={CARD_STYLE} bodyStyle={{ padding: '24px' }}>
            <Statistic
              title={<Text style={{ color: INK_2 }}>Est. cost (today)</Text>}
              value={fmtCost(today)}
              prefix={<DollarOutlined style={{ color: '#5BBFE8', marginRight: 8 }} />}
              valueStyle={{ color: INK, fontSize: 28 }}
            />
            {today && today.unpricedTokens > 0 && (
              <Text style={{ color: INK_2, fontSize: 12 }}>
                +{fmtTokens(today.unpricedTokens)} tokens without pricing
              </Text>
            )}
          </Card>
        </Col>
        <Col xs={24} sm={12} lg={6}>
          <Card style={CARD_STYLE} bodyStyle={{ padding: '24px' }}>
            <Statistic
              title={<Text style={{ color: INK_2 }}>Cache-read share (today)</Text>}
              value={`${cacheShare}%`}
              prefix={<DatabaseOutlined style={{ color: '#5BBFE8', marginRight: 8 }} />}
              valueStyle={{ color: INK, fontSize: 28 }}
            />
          </Card>
        </Col>
      </Row>

      <Card style={{ ...CARD_STYLE, marginTop: 24 }} bodyStyle={{ padding: '24px' }}>
        <Text style={{ color: INK_2 }}>Tokens per day — last 30 days</Text>
        {chartData.length === 0 ? (
          <div
            style={{
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
              height: 220,
            }}
          >
            <Text style={{ color: 'rgba(255,255,255,0.3)' }}>
              No usage recorded yet — data appears after the first LLM calls.
            </Text>
          </div>
        ) : (
          <div style={{ width: '100%', height: 240, marginTop: 12 }}>
            <ResponsiveContainer>
              <AreaChart data={chartData} margin={{ top: 8, right: 8, left: 0, bottom: 0 }}>
                <CartesianGrid stroke={GRID} vertical={false} />
                <XAxis
                  dataKey="date"
                  tick={{ fill: INK_2, fontSize: 11 }}
                  axisLine={{ stroke: GRID }}
                  tickLine={false}
                  minTickGap={24}
                />
                <YAxis
                  tick={{ fill: INK_2, fontSize: 11 }}
                  axisLine={false}
                  tickLine={false}
                  tickFormatter={fmtTokens}
                  width={52}
                />
                <Tooltip
                  formatter={(v, name) => [
                    fmtTokens(Number(v ?? 0)),
                    name === 'in' ? 'Tokens in' : 'Tokens out',
                  ]}
                  contentStyle={{
                    background: '#141428',
                    border: '1px solid rgba(255,255,255,0.1)',
                    borderRadius: 8,
                    color: INK,
                  }}
                  labelStyle={{ color: INK_2 }}
                />
                <Legend
                  formatter={(v: string) => (
                    <span style={{ color: INK_2 }}>{v === 'in' ? 'Tokens in' : 'Tokens out'}</span>
                  )}
                />
                <Area
                  type="monotone"
                  dataKey="in"
                  stroke={C_IN}
                  strokeWidth={2}
                  fill={C_IN}
                  fillOpacity={0.18}
                  dot={false}
                  activeDot={{ r: 4 }}
                />
                <Area
                  type="monotone"
                  dataKey="out"
                  stroke={C_OUT}
                  strokeWidth={2}
                  fill={C_OUT}
                  fillOpacity={0.18}
                  dot={false}
                  activeDot={{ r: 4 }}
                />
              </AreaChart>
            </ResponsiveContainer>
          </div>
        )}
      </Card>

      <Row gutter={[24, 24]} style={{ marginTop: 24 }}>
        <Col xs={24} lg={12}>
          <Card style={CARD_STYLE} bodyStyle={{ padding: '16px 24px 8px' }}>
            <Text style={{ color: INK_2 }}>By model — last 7 days</Text>
            <Table<BreakdownRow>
              size="small"
              rowKey="key"
              columns={breakdownColumns('Model')}
              dataSource={byModel.slice(0, 6)}
              pagination={false}
              locale={{ emptyText: <Text style={{ color: 'rgba(255,255,255,0.3)' }}>No data</Text> }}
              style={{ marginTop: 8 }}
            />
          </Card>
        </Col>
        <Col xs={24} lg={12}>
          <Card style={CARD_STYLE} bodyStyle={{ padding: '16px 24px 8px' }}>
            <Text style={{ color: INK_2 }}>By Space App — last 7 days</Text>
            <Table<BreakdownRow>
              size="small"
              rowKey="key"
              columns={breakdownColumns('App')}
              dataSource={byApp.slice(0, 6)}
              pagination={false}
              locale={{ emptyText: <Text style={{ color: 'rgba(255,255,255,0.3)' }}>No data</Text> }}
              style={{ marginTop: 8 }}
            />
          </Card>
        </Col>
      </Row>
    </div>
  );
}
