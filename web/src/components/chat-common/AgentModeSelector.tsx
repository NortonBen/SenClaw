import { Dropdown, Tag, type MenuProps } from 'antd';
import { ThunderboltOutlined, BulbOutlined, ApartmentOutlined, DownOutlined } from '@ant-design/icons';
import type { AgentMode } from '../../hooks/useWebSocket';

interface Props {
  mode: AgentMode;
  onChange: (mode: AgentMode) => void;
  disabled?: boolean;
  /** When true render as a tag (read-only badge); otherwise dropdown. */
  readOnly?: boolean;
}

/**
 * Agent / Plan / Dag mode toggle for the chat input.
 *
 * - **Agent** — default free-execution state with full tool access.
 * - **Plan** — read-only research → write plan file → approval via `ExitPlanMode`.
 * - **Dag** — orchestrate a team of sub-agents via DAG task graph dispatch.
 */
export function AgentModeSelector({ mode, onChange, disabled, readOnly }: Props) {
  const items: MenuProps['items'] = [
    {
      key: 'Agent',
      icon: <ThunderboltOutlined />,
      label: (
        <div>
          <div style={{ fontWeight: 600 }}>Agent</div>
          <div style={{ fontSize: 11, opacity: 0.7 }}>
            Full tool access. Default working mode.
          </div>
        </div>
      ),
    },
    {
      key: 'Plan',
      icon: <BulbOutlined />,
      label: (
        <div>
          <div style={{ fontWeight: 600 }}>Plan</div>
          <div style={{ fontSize: 11, opacity: 0.7 }}>
            Read-only research → write plan file → request approval.
          </div>
        </div>
      ),
    },
    {
      key: 'Dag',
      icon: <ApartmentOutlined />,
      label: (
        <div>
          <div style={{ fontWeight: 600 }}>DAG Team</div>
          <div style={{ fontSize: 11, opacity: 0.7 }}>
            Orchestrate sub-agents via task graph. No direct edits.
          </div>
        </div>
      ),
    },
  ];

  const tag =
    mode === 'Plan' ? (
      <Tag color="gold" icon={<BulbOutlined />} style={{ margin: 0, cursor: readOnly ? 'default' : 'pointer' }}>
        Plan
      </Tag>
    ) : mode === 'Dag' ? (
      <Tag color="purple" icon={<ApartmentOutlined />} style={{ margin: 0, cursor: readOnly ? 'default' : 'pointer' }}>
        DAG
      </Tag>
    ) : (
      <Tag icon={<ThunderboltOutlined />} style={{ margin: 0, cursor: readOnly ? 'default' : 'pointer' }}>
        Agent
      </Tag>
    );

  if (readOnly) {
    return tag;
  }

  return (
    <Dropdown
      menu={{
        items,
        selectedKeys: [mode],
        onClick: ({ key }) => {
          if (disabled) return;
          if (key === 'Agent' || key === 'Plan' || key === 'Dag') {
            onChange(key);
          }
        },
      }}
      disabled={disabled}
      trigger={['click']}
    >
      <span
        role="button"
        aria-label={`Mode: ${mode}`}
        style={{
          display: 'inline-flex',
          alignItems: 'center',
          gap: 4,
          padding: '2px 6px',
          borderRadius: 6,
          userSelect: 'none',
          cursor: disabled ? 'not-allowed' : 'pointer',
          opacity: disabled ? 0.5 : 1,
        }}
      >
        {tag}
        <DownOutlined style={{ fontSize: 10, opacity: 0.6 }} />
      </span>
    </Dropdown>
  );
}
