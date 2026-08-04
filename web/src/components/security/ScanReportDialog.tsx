/**
 * Review dialog for a pre-install security scan.
 *
 * Shown when an install is refused (HTTP 422 with `blocked: true`) and also
 * when it succeeded with findings below the blocking threshold. The whole
 * point is that a security decision is made by reading the findings, so this
 * renders them as a list — not as a toast full of text.
 *
 * The safe action is the default: "Cancel" is the primary button and the
 * override is a plain danger button that only appears when the install was
 * actually blocked.
 */
import { Modal, List, Tag, Typography, Alert, Button, Space } from 'antd';
import { WarningOutlined } from '@ant-design/icons';

const { Text, Paragraph } = Typography;

export type Severity = 'info' | 'low' | 'medium' | 'high' | 'critical';

export interface ScanFinding {
  rule: string;
  severity: Severity;
  title: string;
  detail: string;
  file: string;
  line?: number | null;
  evidence: string;
}

export interface ScanReport {
  target: string;
  kind: 'plugin' | 'space_app';
  findings: ScanFinding[];
  files_scanned: number;
  truncated: boolean;
}

/** Parse a fetch Response that may carry a blocked-install scan report. */
export async function readScanError(
  res: Response,
): Promise<{ blocked: boolean; error: string; scan?: ScanReport }> {
  const text = await res.text();
  try {
    const body = JSON.parse(text);
    return {
      blocked: body?.blocked === true,
      error: body?.error ?? text,
      scan: body?.scan,
    };
  } catch {
    // Not every failure is a scan block — plain-text errors still surface.
    return { blocked: false, error: text || `${res.status} ${res.statusText}` };
  }
}

const SEVERITY_COLOR: Record<Severity, string> = {
  critical: 'red',
  high: 'volcano',
  medium: 'orange',
  low: 'gold',
  info: 'default',
};

const SEVERITY_RANK: Record<Severity, number> = {
  critical: 4,
  high: 3,
  medium: 2,
  low: 1,
  info: 0,
};

/** Weights mirror `Severity::weight` in src/security/scan.rs. */
const SEVERITY_WEIGHT: Record<Severity, number> = {
  critical: 40,
  high: 20,
  medium: 8,
  low: 3,
  info: 0,
};

export function riskScore(report: ScanReport): number {
  return Math.min(
    100,
    report.findings.reduce((sum, f) => sum + (SEVERITY_WEIGHT[f.severity] ?? 0), 0),
  );
}

interface Props {
  open: boolean;
  report?: ScanReport;
  /** Name shown in the title; falls back to the report's own target. */
  target?: string;
  /** True when the install was refused — enables the override button. */
  blocked: boolean;
  busy?: boolean;
  onCancel: () => void;
  /** Only wired when `blocked`; re-runs the install with force. */
  onForceInstall?: () => void;
}

export default function ScanReportDialog({
  open,
  report,
  target,
  blocked,
  busy,
  onCancel,
  onForceInstall,
}: Props) {
  if (!report) return null;

  const sorted = [...report.findings].sort(
    (a, b) =>
      SEVERITY_RANK[b.severity] - SEVERITY_RANK[a.severity] || a.rule.localeCompare(b.rule),
  );
  const score = riskScore(report);
  const name = target ?? report.target;

  return (
    <Modal
      open={open}
      onCancel={onCancel}
      width={720}
      title={
        <Space>
          <WarningOutlined style={{ color: blocked ? '#cf1322' : '#d46b08' }} />
          <span>
            Security scan — {name} (risk {score}/100)
          </span>
        </Space>
      }
      footer={
        <Space>
          {blocked && onForceInstall && (
            <Button danger loading={busy} onClick={onForceInstall}>
              Install anyway
            </Button>
          )}
          <Button type="primary" onClick={onCancel}>
            {blocked ? 'Cancel' : 'Close'}
          </Button>
        </Space>
      }
    >
      <Alert
        type={blocked ? 'error' : 'warning'}
        showIcon
        style={{ marginBottom: 12 }}
        message={
          blocked
            ? 'This package was not installed'
            : 'Installed, but the scan flagged the following'
        }
        description={
          blocked
            ? 'Nothing was recorded or enabled. Review the findings below — installing anyway runs this code with the daemon’s privileges.'
            : 'These findings were below the blocking threshold. They are worth reading before you use the package.'
        }
      />

      <List
        size="small"
        dataSource={sorted}
        renderItem={(f) => (
          <List.Item style={{ display: 'block' }}>
            <Space size={6} wrap style={{ marginBottom: 2 }}>
              <Tag color={SEVERITY_COLOR[f.severity]}>{f.severity.toUpperCase()}</Tag>
              <Text strong>{f.title}</Text>
              <Text type="secondary" style={{ fontSize: 12 }}>
                [{f.rule}]
              </Text>
            </Space>
            <Paragraph style={{ margin: '2px 0' }}>{f.detail}</Paragraph>
            <Text type="secondary" style={{ fontSize: 12 }}>
              {f.file}
              {f.line ? `:${f.line}` : ''}
            </Text>
            {/* Rendered as text inside <code>, never as markup — evidence is
                attacker-controlled and must not be interpreted by the page. */}
            <pre
              style={{
                margin: '4px 0 0',
                padding: '6px 8px',
                fontSize: 12,
                whiteSpace: 'pre-wrap',
                wordBreak: 'break-all',
                background: 'rgba(128,128,128,0.12)',
                borderRadius: 4,
              }}
            >
              {f.evidence}
            </pre>
          </List.Item>
        )}
      />

      {report.truncated && (
        <Alert
          type="info"
          showIcon
          style={{ marginTop: 12 }}
          message="Coverage was partial"
          description="The package exceeded the scan budget, so unscanned files may contain further issues."
        />
      )}
    </Modal>
  );
}
