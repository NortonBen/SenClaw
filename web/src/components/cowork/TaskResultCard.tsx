import React, { useState } from 'react';
import {
  Card, Tag, Typography, Collapse, Space, Button, Tooltip, Badge, List, Alert,
} from 'antd';
import {
  CheckCircleOutlined, ClockCircleOutlined, LoadingOutlined,
  CodeOutlined, FileTextOutlined, CopyOutlined, CheckOutlined, CloseOutlined,
} from '@ant-design/icons';
import type { CoworkTask, OutputValidation } from '../../types';

const { Text, Paragraph } = Typography;

const STATUS_COLOR: Record<string, string> = {
  backlog:     '#8c8c8c',
  todo:        '#1890ff',
  in_progress: '#faad14',
  review:      '#722ed1',
  done:        '#52c41a',
  blocked:     '#ff4d4f',
};

const STATUS_ICON: Record<string, React.ReactNode> = {
  in_progress: <LoadingOutlined spin />,
  done:        <CheckCircleOutlined />,
  blocked:     <ClockCircleOutlined />,
};

interface TaskResultCardProps {
  task: CoworkTask;
  highlight?: boolean;
  outputValidation?: OutputValidation | null;
}

export function TaskResultCard({ task, highlight, outputValidation }: TaskResultCardProps) {
  const [copied, setCopied] = useState(false);

  const artifacts: string[] = (() => {
    try { return task.artifacts ? JSON.parse(task.artifacts) : []; }
    catch { return []; }
  })();

  const refs: string[] = (() => {
    try { return task.references ? JSON.parse(task.references) : []; }
    catch { return []; }
  })();

  const copyResult = () => {
    if (!task.resultOutput?.trim()) return;
    navigator.clipboard.writeText(task.resultOutput).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    });
  };

  const collapseItems = [];

  if (task.resultOutput && task.resultOutput.trim()) {
    collapseItems.push({
      key: 'result',
      label: (
        <Space>
          <FileTextOutlined />
          <Text strong>Kết quả</Text>
          {task.resultOutput.length > 200 && (
            <Text type="secondary" style={{ fontSize: 11 }}>
              {task.resultOutput.length} ký tự
            </Text>
          )}
        </Space>
      ),
      extra: (
        <Tooltip title={copied ? 'Đã copy!' : 'Copy kết quả'}>
          <Button
            size="small"
            type="text"
            icon={<CopyOutlined />}
            onClick={e => { e.stopPropagation(); copyResult(); }}
          />
        </Tooltip>
      ),
      children: (
        <Paragraph
          style={{
            whiteSpace: 'pre-wrap',
            fontFamily: 'monospace',
            fontSize: 12,
            background: '#fafafa',
            padding: 12,
            borderRadius: 4,
            maxHeight: 400,
            overflow: 'auto',
            margin: 0,
          }}
        >
          {task.resultOutput}
        </Paragraph>
      ),
    });
  }

  if (artifacts.length > 0) {
    collapseItems.push({
      key: 'artifacts',
      label: (
        <Space>
          <CodeOutlined />
          <Text strong>Files tạo ra</Text>
          <Badge count={artifacts.length} color="blue" />
        </Space>
      ),
      children: (
        <ul style={{ margin: 0, padding: '0 0 0 16px' }}>
          {artifacts.map(a => (
            <li key={a}>
              <Text code style={{ fontSize: 11 }}>{a}</Text>
            </li>
          ))}
        </ul>
      ),
    });
  }

  if (refs.length > 0) {
    collapseItems.push({
      key: 'refs',
      label: (
        <Space>
          <FileTextOutlined />
          <Text strong>Tài liệu tham khảo</Text>
          <Badge count={refs.length} color="purple" />
        </Space>
      ),
      children: (
        <ul style={{ margin: 0, padding: '0 0 0 16px' }}>
          {refs.map(r => (
            <li key={r}>
              <Text code style={{ fontSize: 11 }}>{r}</Text>
            </li>
          ))}
        </ul>
      ),
    });
  }

  // Output validation analysis
  if (outputValidation) {
    const { formatValid, expectedFormat, requiredSectionsPresent, requiredSectionsMissing, overallCompliant } = outputValidation;
    collapseItems.push({
      key: 'validation',
      label: (
        <Space>
          {overallCompliant ? <CheckOutlined style={{ color: '#52c41a' }} /> : <CloseOutlined style={{ color: '#ff4d4f' }} />}
          <Text strong>Output Validation</Text>
          <Badge count={overallCompliant ? 'Compliant' : 'Non-compliant'} color={overallCompliant ? 'green' : 'red'} />
        </Space>
      ),
      children: (
        <div style={{ fontSize: 12 }}>
          {expectedFormat && (
            <div style={{ marginBottom: 8 }}>
              <Text type="secondary">Format: </Text>
              <Tag color={formatValid ? 'green' : 'red'} style={{ marginLeft: 4 }}>
                {expectedFormat} {formatValid ? '✓' : '✗'}
              </Tag>
            </div>
          )}
          {requiredSectionsPresent.length > 0 && (
            <div style={{ marginBottom: 8 }}>
              <Text type="secondary">Required sections present: </Text>
              <div style={{ marginTop: 4 }}>
                {requiredSectionsPresent.map(section => (
                  <Tag key={section} color="green" style={{ margin: '2px' }}>{section} ✓</Tag>
                ))}
              </div>
            </div>
          )}
          {requiredSectionsMissing.length > 0 && (
            <div>
              <Text type="secondary">Required sections missing: </Text>
              <div style={{ marginTop: 4 }}>
                {requiredSectionsMissing.map(section => (
                  <Tag key={section} color="red" style={{ margin: '2px' }}>{section} ✗</Tag>
                ))}
              </div>
            </div>
          )}
          {!overallCompliant && (
            <Alert
              message="Output does not meet all requirements"
              type="warning"
              showIcon
              style={{ marginTop: 12, fontSize: 11 }}
            />
          )}
        </div>
      ),
    });
  }

  return (
    <Card
      size="small"
      style={{
        border: highlight ? '2px solid #52c41a' : undefined,
        transition: 'border-color 0.3s',
      }}
      title={
        <Space>
          {STATUS_ICON[task.status]}
          <Text strong style={{ fontSize: 13 }}>{task.title}</Text>
          <Tag color={STATUS_COLOR[task.status]} style={{ margin: 0 }}>
            {task.status.replace('_', ' ')}
          </Tag>
          {task.priority !== 'medium' && (
            <Tag color={task.priority === 'critical' ? 'red' : task.priority === 'high' ? 'orange' : 'default'}>
              {task.priority}
            </Tag>
          )}
        </Space>
      }
      extra={
        task.completedAt && (
          <Text type="secondary" style={{ fontSize: 11 }}>
            {new Date(task.completedAt).toLocaleString()}
          </Text>
        )
      }
    >
      {task.inputSummary && (
        <div style={{ marginBottom: 8 }}>
          <Text type="secondary" style={{ fontSize: 11 }}>Input: </Text>
          <Text style={{ fontSize: 12 }} ellipsis={{ tooltip: task.inputSummary }}>
            {task.inputSummary.slice(0, 120)}{task.inputSummary.length > 120 ? '…' : ''}
          </Text>
        </div>
      )}

      {collapseItems.length > 0 ? (
        <Collapse
          size="small"
          ghost
          defaultActiveKey={task.status === 'done' ? ['result'] : []}
          items={collapseItems}
        />
      ) : task.status === 'in_progress' ? (
        <Text type="secondary" style={{ fontSize: 12 }}>
          <LoadingOutlined spin style={{ marginRight: 6 }} />
          Đang xử lý…
        </Text>
      ) : task.status === 'done' ? (
        <Text type="secondary" style={{ fontSize: 12 }}>
          <CheckCircleOutlined style={{ marginRight: 6, color: '#52c41a' }} />
          Hoàn thành — kết quả chưa được ghi lại
        </Text>
      ) : (
        <Text type="secondary" style={{ fontSize: 12 }}>Chưa có kết quả</Text>
      )}
    </Card>
  );
}
