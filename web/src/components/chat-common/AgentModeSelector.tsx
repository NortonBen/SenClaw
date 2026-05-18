import { Dropdown, Tag, type MenuProps } from 'antd';
import { ThunderboltOutlined, BulbOutlined, DownOutlined } from '@ant-design/icons';
import type { AgentMode } from '../../hooks/useWebSocket';

interface Props {
  mode: AgentMode;
  onChange: (mode: AgentMode) => void;
  disabled?: boolean;
  /** When true render as a tag (read-only badge); otherwise dropdown. */
  readOnly?: boolean;
}

/**
 * Agent / Plan mode toggle for the chat input.
 *
 * Plan mode constrains the agent to writing a plan file and routes approval
 * through `ExitPlanMode` — see `code-old/sema-code-core/prompt/plan.ts`.
 * Agent mode is the default free-execution state.
 *
 * Click → emits `agent:mode` WS to `setAgentMode(jid, mode)` (in useWebSocket).
 * Backend `ZenCoreApi::update_agent_mode` flips `ZenCoreOptions.agent_mode`
 * which `tools_for_main_agent()` consumes to strip `TodoWrite` and to gate
 * write tools through the system reminder.
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
  ];

  const tag =
    mode === 'Plan' ? (
      <Tag color="gold" icon={<BulbOutlined />} style={{ margin: 0, cursor: readOnly ? 'default' : 'pointer' }}>
        Plan
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
          if (key === 'Agent' || key === 'Plan') {
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
