import React from 'react';
import {
  Avatar, Card, Tag, Typography, Space, Button, Popconfirm, Divider, Tooltip,
} from 'antd';
import {
  RobotOutlined, CrownOutlined, EditOutlined, DeleteOutlined,
  ArrowRightOutlined, CheckCircleOutlined, ThunderboltOutlined,
  FileTextOutlined, ClockCircleOutlined, ToolOutlined,
} from '@ant-design/icons';
import type { CoworkMember } from '../../types';

const { Text } = Typography;

function parseJson<T>(s: string | null | undefined, fallback: T): T {
  if (!s) return fallback;
  try { return JSON.parse(s) as T; } catch { return fallback; }
}

interface SectionProps {
  icon: React.ReactNode;
  label: string;
  children: React.ReactNode;
}

function Section({ icon, label, children }: SectionProps) {
  return (
    <div style={{ marginBottom: 10 }}>
      <Space size={4} style={{ marginBottom: 4 }}>
        <span style={{ color: '#8c8c8c', fontSize: 10 }}>{icon}</span>
        <Text type="secondary" style={{ fontSize: 10, textTransform: 'uppercase', letterSpacing: 0.8, fontWeight: 600 }}>
          {label}
        </Text>
      </Space>
      <div>{children}</div>
    </div>
  );
}

interface AgentCardProps {
  member: CoworkMember;
  onEdit: (m: CoworkMember) => void;
  onRemove: (memberId: string) => void;
}

export function AgentCard({ member: m, onEdit, onRemove }: AgentCardProps) {
  const triggers = parseJson<Record<string, unknown>[]>(m.triggers, []);
  const handoffRules = parseJson<Record<string, unknown>[]>(m.handoffRules, []);
  const criteria = parseJson<string[]>(m.acceptanceCriteria, []);
  const responsibilities = parseJson<string[]>(m.responsibilities, []);

  const output = parseJson<{
    format?: string;
    requiredSections?: string[];
    attachDiff?: boolean;
  }>(m.outputFormat, {});

  const sla = parseJson<{
    maxDurationPerTaskMinutes?: number;
    maxTokenPerTask?: number;
    escalateAfterBlockedMinutes?: number;
  }>(m.sla, {});

  const limits = parseJson<{
    deniedTools?: string[];
    allowedBashCommands?: string[];
    maxFileSizeWriteKb?: number;
  }>(m.limits, {});

  const roleColor = m.role === 'lead' ? '#faad14' : m.role === 'reviewer' ? '#722ed1' : '#1890ff';
  const roleTagColor = m.role === 'lead' ? 'gold' : m.role === 'reviewer' ? 'purple' : 'blue';

  return (
    <Card
      size="small"
      style={{ borderRadius: 10, height: '100%', border: `1px solid ${roleColor}22` }}
      styles={{ header: { borderBottom: `2px solid ${roleColor}33` } }}
      title={
        <Space>
          <Avatar
            icon={m.role === 'lead' ? <CrownOutlined /> : <RobotOutlined />}
            size={28}
            style={{ backgroundColor: roleColor }}
          />
          <Text strong style={{ fontSize: 13 }}>{m.memberId}</Text>
          {m.subdir && (
            <Text code style={{ fontSize: 10 }}>{m.subdir}/</Text>
          )}
          <Tag color={roleTagColor} icon={m.role === 'lead' ? <CrownOutlined /> : undefined} style={{ fontSize: 10, margin: 0 }}>
            {m.role}
          </Tag>
        </Space>
      }
      extra={
        <Space size={0}>
          <Button type="text" size="small" icon={<EditOutlined style={{ fontSize: 11 }} />} onClick={() => onEdit(m)} />
          <Popconfirm title="Remove this agent from workspace?" onConfirm={() => onRemove(m.memberId)}>
            <Button type="text" size="small" danger icon={<DeleteOutlined style={{ fontSize: 11 }} />} onClick={e => e.stopPropagation()} />
          </Popconfirm>
        </Space>
      }
    >
      {/* Persona */}
      {m.persona && (
        <div style={{ marginBottom: 10, padding: '6px 8px', background: '#fafafa', borderRadius: 6, borderLeft: `3px solid ${roleColor}` }}>
          <Text style={{ fontSize: 12, fontStyle: 'italic', color: '#595959' }}>{m.persona}</Text>
        </div>
      )}

      {/* Responsibilities */}
      {responsibilities.length > 0 && (
        <Section icon={<CheckCircleOutlined />} label="Responsibilities">
          <ul style={{ margin: 0, padding: '0 0 0 16px' }}>
            {responsibilities.map((r, i) => (
              <li key={i} style={{ fontSize: 11, color: '#434343', marginBottom: 1 }}>{r}</li>
            ))}
          </ul>
        </Section>
      )}

      {/* Triggers */}
      {triggers.length > 0 && (
        <Section icon={<ThunderboltOutlined />} label="Triggers">
          <Space size={4} wrap>
            {triggers.map((t, i) => {
              const label = [
                t.type as string,
                t.condition ? `: ${t.condition}` : '',
                t.from ? ` from ${t.from as string}` : '',
                t.messageType ? `[${t.messageType as string}]` : '',
              ].join('');
              return (
                <Tag key={i} color="purple" style={{ fontSize: 10, margin: 0 }}>{label}</Tag>
              );
            })}
          </Space>
        </Section>
      )}

      {/* Handoff Rules */}
      {handoffRules.length > 0 && (
        <Section icon={<ArrowRightOutlined />} label="Handoff Rules">
          <Space direction="vertical" size={2} style={{ width: '100%' }}>
            {handoffRules.map((h, i) => (
              <Space key={i} size={4} align="center">
                <Tag color="orange" style={{ fontSize: 10, margin: 0 }}>{h.when as string}</Tag>
                <ArrowRightOutlined style={{ fontSize: 9, color: '#8c8c8c' }} />
                <Text strong style={{ fontSize: 11 }}>{h.to as string}</Text>
                <Tag style={{ fontSize: 10, margin: 0 }}>({h.type as string})</Tag>
              </Space>
            ))}
          </Space>
        </Section>
      )}

      {/* Acceptance Criteria */}
      {criteria.length > 0 && (
        <Section icon={<CheckCircleOutlined />} label="Acceptance Criteria">
          {criteria.map((c, i) => (
            <div key={i} style={{ fontSize: 11, marginBottom: 2, color: '#434343' }}>
              <CheckCircleOutlined style={{ color: '#52c41a', marginRight: 5, fontSize: 10 }} />
              {c}
            </div>
          ))}
        </Section>
      )}

      {/* Output */}
      {m.outputFormat && (output.format || (output.requiredSections?.length ?? 0) > 0) && (
        <>
          <Divider style={{ margin: '8px 0' }} />
          <Section icon={<FileTextOutlined />} label="Output">
            <Space size={4} wrap>
              {output.format && (
                <Tag color="geekblue" style={{ fontSize: 10, margin: 0 }}>{output.format}</Tag>
              )}
              {output.attachDiff && (
                <Tag color="green" style={{ fontSize: 10, margin: 0 }}>+diff</Tag>
              )}
              {output.requiredSections?.map((s, i) => (
                <Tag key={i} style={{ fontSize: 10, margin: 0 }}>{s}</Tag>
              ))}
            </Space>
          </Section>
        </>
      )}

      {/* SLA + Limits row */}
      {(m.sla || m.limits) && (
        <Divider style={{ margin: '8px 0' }} />
      )}
      <Space size={16} wrap>
        {m.sla && (
          <Section icon={<ClockCircleOutlined />} label="SLA">
            <Space size={4} wrap>
              {sla.maxDurationPerTaskMinutes && (
                <Tag color="orange" style={{ fontSize: 10, margin: 0 }}>{sla.maxDurationPerTaskMinutes}min</Tag>
              )}
              {sla.maxTokenPerTask && (
                <Tooltip title="Max tokens per task">
                  <Tag color="orange" style={{ fontSize: 10, margin: 0 }}>{(sla.maxTokenPerTask / 1000).toFixed(0)}k tokens</Tag>
                </Tooltip>
              )}
              {sla.escalateAfterBlockedMinutes && (
                <Tag color="red" style={{ fontSize: 10, margin: 0 }}>escalate {sla.escalateAfterBlockedMinutes}min</Tag>
              )}
            </Space>
          </Section>
        )}
        {m.limits && (
          <Section icon={<ToolOutlined />} label="Limits">
            <Space size={4} wrap>
              {limits.deniedTools?.map((t, i) => (
                <Tag key={i} color="red" style={{ fontSize: 10, margin: 0 }}>no {t}</Tag>
              ))}
              {limits.allowedBashCommands?.map((c, i) => (
                <Tag key={i} color="green" style={{ fontSize: 10, margin: 0 }}>{c}</Tag>
              ))}
              {limits.maxFileSizeWriteKb && (
                <Tag color="gold" style={{ fontSize: 10, margin: 0 }}>max {limits.maxFileSizeWriteKb}KB</Tag>
              )}
            </Space>
          </Section>
        )}
      </Space>
    </Card>
  );
}
