import { useCallback, useEffect, useRef, useState } from 'react'
import {
  Alert,
  App as AntApp,
  Button,
  Empty,
  Popconfirm,
  Col,
  Row,
  Space,
  Statistic,
  Switch,
  Table,
  Tag,
  Typography,
} from 'antd'
import { PoweroffOutlined, ReloadOutlined, StopOutlined } from '@ant-design/icons'
import { api, type Proc, type Sandbox, type Stats } from './api'
import { AreaSpark, ChartHeader, SERIES } from './chart'
import type { Resolved } from './theme'
import { useT } from './i18n'

const POLL_MS = 2000
/** Samples kept for the charts — 90 × 2 s ≈ 3 minutes of history. */
const HISTORY = 90

/**
 * Round a peak up to a stable axis ceiling.
 *
 * The alternative — scaling the axis to the current peak on every tick — makes
 * a flat workload look like it is climbing, because the axis moves under it.
 * Snapping to a step means the axis only changes when the data genuinely
 * outgrows it.
 */
function ceilingFor(peak: number, floor: number): number {
  const target = Math.max(peak * 1.15, floor)
  const step = Math.pow(10, Math.floor(Math.log10(Math.max(target, 1))))
  return Math.max(floor, Math.ceil(target / step) * step)
}

/** CPU/RAM and the process list for one sandbox, with a way to stop things. */
export function MonitorPanel({ sandbox, mode }: { sandbox: Sandbox; mode: Resolved }) {
  const { message } = AntApp.useApp()
  const t = useT()
  const [stats, setStats] = useState<Stats | null>(null)
  const [live, setLive] = useState(true)
  const [busy, setBusy] = useState(false)
  const [cpuHist, setCpuHist] = useState<number[]>([])
  const [ramHist, setRamHist] = useState<number[]>([])
  // Held in a ref so the polling effect does not restart on every tick.
  const liveRef = useRef(live)
  liveRef.current = live

  const load = useCallback(async () => {
    try {
      const s = await api.stats(sandbox.id)
      setStats(s)
      setCpuHist((h) => [...h, s.cpu].slice(-HISTORY))
      setRamHist((h) => [...h, s.rssMb].slice(-HISTORY))
    } catch (e) {
      message.error((e as Error).message)
      setLive(false)
    }
  }, [sandbox.id, message])

  // History belongs to one sandbox; carrying it across a switch would draw
  // another sandbox's load as if it were this one's.
  useEffect(() => {
    setCpuHist([])
    setRamHist([])
  }, [sandbox.id])

  useEffect(() => {
    void load()
    // A single interval that checks the toggle, rather than tearing the timer
    // down and up as the switch flips.
    const t = setInterval(() => {
      if (liveRef.current) void load()
    }, POLL_MS)
    return () => clearInterval(t)
  }, [load])

  const killOne = async (p: Proc) => {
    try {
      await api.kill(sandbox.id, p.pid)
      message.success(`${t.stop} ${p.pid}`)
      void load()
    } catch (e) {
      message.error((e as Error).message)
    }
  }

  const killAll = async () => {
    setBusy(true)
    try {
      await api.kill(sandbox.id)
      message.success(t.stopAll)
      void load()
    } catch (e) {
      message.error((e as Error).message)
    } finally {
      setBusy(false)
    }
  }

  // A 100 % floor keeps a lightly loaded sandbox from filling the plot; RAM has
  // no natural ceiling, so the configured limit (docker) or a 64 MB floor.
  const cpuCeiling = ceilingFor(Math.max(...cpuHist, 0), 100)
  const ramCeiling = ceilingFor(
    Math.max(...ramHist, 0),
    stats?.memoryLimitMb ?? 64,
  )

  const columns = [
    { title: 'PID', dataIndex: 'pid', width: 80 },
    {
      title: '%CPU',
      dataIndex: 'cpu',
      width: 90,
      render: (v: number) => v.toFixed(1),
    },
    {
      title: t.ram,
      dataIndex: 'rssMb',
      width: 110,
      render: (v: number) => `${v.toFixed(1)} MB`,
    },
    { title: t.colElapsed, dataIndex: 'elapsed', width: 110 },
    {
      title: t.colCommand,
      dataIndex: 'command',
      render: (v: string) => (
        <Typography.Text className="sbx-mono" ellipsis={{ tooltip: v }}>
          {v}
        </Typography.Text>
      ),
    },
    {
      title: '',
      width: 60,
      render: (_: unknown, p: Proc) => (
        <Popconfirm
          title={t.stopProcess(p.pid)}
          okText={t.stop}
          cancelText={t.cancel}
          onConfirm={() => void killOne(p)}
        >
          <Button size="small" type="text" danger icon={<StopOutlined />} />
        </Popconfirm>
      ),
    },
  ]

  return (
    <Space direction="vertical" style={{ width: '100%' }} size={14}>
      <Space wrap size={24}>
        <Statistic
          title={t.cpu}
          value={stats?.cpu ?? 0}
          precision={1}
          suffix="%"
          valueStyle={{ color: (stats?.cpu ?? 0) > 80 ? '#e05252' : undefined }}
        />
        <Statistic
          title={t.ram}
          value={stats?.rssMb ?? 0}
          precision={1}
          suffix={
            stats?.memoryLimitMb ? `/ ${stats.memoryLimitMb} MB` : 'MB'
          }
        />
        <Statistic title={t.processes} value={stats?.processes.length ?? 0} />
      </Space>

      {/* Two charts, not two lines on one plot: % and MB are different units on
          different scales, and a shared axis would invite reading a
          relationship out of where the two lines cross. */}
      <Row gutter={[16, 16]}>
        <Col xs={24} lg={12}>
          <ChartHeader
            title={t.cpuOverTime}
            value={`${(stats?.cpu ?? 0).toFixed(1)} %`}
            color={SERIES.cpu[mode]}
            ceilingLabel={t.axisTop(`${cpuCeiling.toFixed(0)} %`)}
          />
          <AreaSpark
            points={cpuHist}
            ceiling={cpuCeiling}
            color={SERIES.cpu[mode]}
            mode={mode}
            sampleMs={POLL_MS}
            format={(v) => `${v.toFixed(1)} %`}
          />
        </Col>
        <Col xs={24} lg={12}>
          <ChartHeader
            title={t.ramOverTime}
            value={`${(stats?.rssMb ?? 0).toFixed(1)} MB`}
            color={SERIES.ram[mode]}
            ceilingLabel={t.axisTop(`${ramCeiling.toFixed(0)} MB`)}
          />
          <AreaSpark
            points={ramHist}
            ceiling={ramCeiling}
            color={SERIES.ram[mode]}
            mode={mode}
            sampleMs={POLL_MS}
            format={(v) => `${v.toFixed(1)} MB`}
          />
        </Col>
      </Row>
      {cpuHist.length < 2 && (
        <Typography.Text type="secondary" style={{ fontSize: 12 }}>
          {t.chartsBuilding(POLL_MS / 1000)}
        </Typography.Text>
      )}

      <Space wrap>
        <Button size="small" icon={<ReloadOutlined />} onClick={() => void load()}>
          {t.refresh}
        </Button>
        <Space size={6}>
          <Switch size="small" checked={live} onChange={setLive} />
          <Typography.Text type="secondary">{t.autoRefresh}</Typography.Text>
        </Space>
        <Popconfirm
          title={t.stopAllConfirm}
          description={
            sandbox.backend === 'docker'
              ? t.stopAllDocker
              : t.stopAllDirect
          }
          okText={t.stopAll}
          cancelText={t.cancel}
          onConfirm={() => void killAll()}
        >
          <Button size="small" danger icon={<PoweroffOutlined />} loading={busy}>
            {t.stopAll}
          </Button>
        </Popconfirm>
        {stats && (
          <Tag color={stats.running ? 'green' : 'default'}>
            {stats.running ? t.running : t.idle}
          </Tag>
        )}
      </Space>

      {stats?.note && <Alert type="warning" showIcon message={stats.note} />}

      {/* `direct` inherits the host's memory — saying so beats showing a RAM
          number next to a limit that is not actually enforced. */}
      {stats?.source === 'host' && (
        <Alert
          type="info"
          showIcon
          message={t.noRamCeiling}
        />
      )}

      {stats && stats.processes.length === 0 ? (
        <Empty
          description={t.idleEmpty}
          image={Empty.PRESENTED_IMAGE_SIMPLE}
        />
      ) : (
        <Table
          size="small"
          rowKey="pid"
          pagination={false}
          dataSource={stats?.processes ?? []}
          columns={columns}
          scroll={{ x: true }}
        />
      )}
    </Space>
  )
}
