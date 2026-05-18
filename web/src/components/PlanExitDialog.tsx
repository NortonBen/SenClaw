import { Modal, Button, Typography, Space, Tag } from 'antd';
import { FileTextOutlined, CheckCircleOutlined, RedoOutlined } from '@ant-design/icons';
import ReactMarkdown from 'react-markdown';
import type { PlanExitOption, PlanExitRequest } from '../hooks/useWebSocket';

const { Text } = Typography;

interface Props {
  request: PlanExitRequest | null;
  onResolve: (selected: PlanExitOption) => void;
  onDismiss: () => void;
}

/**
 * Render the plan-approval modal that opens when an agent in Plan mode calls
 * the `ExitPlanMode` tool. Mirrors the sema-core `plan_to_agent` UX:
 *   - User sees the full plan markdown rendered
 *   - Two approval options: "Approve & start editing" / "Clear context & start fresh"
 *
 * Mount once at the app root and pass `ws.planExitRequest` plus
 * `ws.resolvePlanExit` / `ws.dismissPlanExit` from `useWebSocket`.
 */
export function PlanExitDialog({ request, onResolve, onDismiss }: Props) {
  const open = request !== null;

  return (
    <Modal
      title={
        <Space>
          <FileTextOutlined />
          <span>Plan ready for review</span>
          {request?.planFilePath && (
            <Tag color="blue" style={{ fontSize: 11 }}>
              {shortPath(request.planFilePath)}
            </Tag>
          )}
        </Space>
      }
      open={open}
      onCancel={onDismiss}
      width={780}
      destroyOnClose
      footer={[
        <Button key="dismiss" onClick={onDismiss}>
          Dismiss
        </Button>,
        <Button
          key="restart"
          icon={<RedoOutlined />}
          onClick={() => onResolve('clearContextAndStart')}
        >
          {request?.options.clearContextAndStart ?? 'Clear context and start fresh'}
        </Button>,
        <Button
          key="approve"
          type="primary"
          icon={<CheckCircleOutlined />}
          onClick={() => onResolve('startEditing')}
        >
          {request?.options.startEditing ?? 'Approve plan and start editing'}
        </Button>,
      ]}
    >
      {request && (
        <>
          <Text type="secondary" style={{ fontSize: 12 }}>
            The agent has finished planning. Review the proposal and choose how to
            continue. <strong>Approve & start editing</strong> keeps the current
            conversation; <strong>clear context</strong> starts a fresh session
            using only the plan file as input.
          </Text>
          <div
            style={{
              marginTop: 16,
              padding: '12px 16px',
              background: '#fafafa',
              border: '1px solid #f0f0f0',
              borderRadius: 6,
              maxHeight: 480,
              overflow: 'auto',
              fontSize: 13,
            }}
          >
            <ReactMarkdown>{request.planContent}</ReactMarkdown>
          </div>
        </>
      )}
    </Modal>
  );
}

function shortPath(p: string): string {
  if (p.length <= 60) return p;
  const parts = p.split(/[\\/]/);
  if (parts.length <= 2) return `…${p.slice(-58)}`;
  return `…/${parts.slice(-2).join('/')}`;
}
