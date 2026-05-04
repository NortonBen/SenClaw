import { useState, useCallback } from 'react';
import {
  theme,
  Typography,
  Button,
  Switch,
  Select,
  Input,
  Space,
  Tag,
  Tooltip,
  Popconfirm,
  Divider,
  Alert,
} from 'antd';
import {
  PlusOutlined,
  DeleteOutlined,
  ThunderboltOutlined,
  CodeOutlined,
  ApiOutlined,
  FileOutlined,
  CloseOutlined,
  CheckOutlined,
  StopOutlined,
  ExclamationCircleOutlined,
  WarningOutlined,
} from '@ant-design/icons';
import type { ToolAutoAcceptRule, RuleMatcher, RuleAction, RuleMatcherType, ToolCategory } from '../types';

const { Text } = Typography;
const { Option } = Select;

// ─── helpers ────────────────────────────────────────────────────────────────

function ruleLabel(rule: ToolAutoAcceptRule): string {
  const m = rule.matcher;
  switch (m.type) {
    case 'bash_glob':   return `Bash: ${m.pattern}`;
    case 'bash_regex':  return `Bash ~/${m.pattern}/`;
    case 'tool_exact':  return `Tool: ${m.tool_name}`;
    case 'mcp_server':  return m.tool ? `MCP: ${m.server} › ${m.tool}` : `MCP: ${m.server}/*`;
    case 'mcp_glob':    return `MCP: ${m.pattern}`;
    case 'tool_category': return `Category: ${m.category}`;
    case 'always':      return 'Tất cả tool';
    default:            return 'Unknown';
  }
}

function ruleIcon(type: RuleMatcherType) {
  switch (type) {
    case 'bash_glob':
    case 'bash_regex':    return <CodeOutlined />;
    case 'mcp_server':
    case 'mcp_glob':      return <ApiOutlined />;
    case 'tool_exact':
    case 'tool_category': return <FileOutlined />;
    case 'always':        return <ThunderboltOutlined />;
    default:              return <FileOutlined />;
  }
}

const ACTION_CONFIG: Record<RuleAction, { label: string; color: string; icon: React.ReactNode }> = {
  auto_accept:          { label: 'Auto Accept',  color: 'success', icon: <CheckOutlined /> },
  auto_accept_and_allow:{ label: 'Accept + Save', color: 'cyan',    icon: <CheckOutlined /> },
  auto_deny:            { label: 'Auto Deny',    color: 'error',   icon: <StopOutlined />  },
  force_request:        { label: 'Force Ask',    color: 'warning', icon: <ExclamationCircleOutlined /> },
};

function generateId() {
  return Math.random().toString(36).slice(2, 8);
}

// ─── Add Rule Form ───────────────────────────────────────────────────────────

interface AddRuleFormProps {
  onAdd: (rule: ToolAutoAcceptRule) => void;
  onCancel: () => void;
  token: any;
}

function AddRuleForm({ onAdd, onCancel, token }: AddRuleFormProps) {
  const [matcherType, setMatcherType] = useState<RuleMatcherType>('bash_glob');
  const [pattern,     setPattern]     = useState('');
  const [toolName,    setToolName]    = useState('');
  const [mcpServer,   setMcpServer]   = useState('');
  const [mcpTool,     setMcpTool]     = useState('');
  const [category,    setCategory]    = useState<ToolCategory>('bash');
  const [action,      setAction]      = useState<RuleAction>('auto_accept');
  const [description, setDescription] = useState('');

  const buildMatcher = (): RuleMatcher => {
    switch (matcherType) {
      case 'bash_glob':    return { type: 'bash_glob', pattern };
      case 'bash_regex':   return { type: 'bash_regex', pattern };
      case 'mcp_glob':     return { type: 'mcp_glob', pattern };
      case 'tool_exact':   return { type: 'tool_exact', tool_name: toolName };
      case 'mcp_server':   return { type: 'mcp_server', server: mcpServer, tool: mcpTool || null };
      case 'tool_category':return { type: 'tool_category', category };
      case 'always':       return { type: 'always' };
      default:             return { type: 'bash_glob', pattern };
    }
  };

  const isValid = (): boolean => {
    switch (matcherType) {
      case 'bash_glob':
      case 'bash_regex':
      case 'mcp_glob':    return pattern.trim().length > 0;
      case 'tool_exact':  return toolName.trim().length > 0;
      case 'mcp_server':  return mcpServer.trim().length > 0;
      default:            return true;
    }
  };

  const handleAdd = () => {
    if (!isValid()) return;
    const rule: ToolAutoAcceptRule = {
      id: generateId(),
      matcher: buildMatcher(),
      action,
      enabled: true,
      description: description.trim() || null,
    };
    onAdd(rule);
  };

  return (
    <div style={{
      background: token.colorFillAlter,
      borderRadius: 8,
      padding: 12,
      border: `1px solid ${token.colorBorderSecondary}`,
      display: 'flex',
      flexDirection: 'column',
      gap: 8,
    }}>
      <Text style={{ fontSize: 11, fontWeight: 600, color: token.colorTextSecondary }}>Loại điều kiện</Text>
      <Select
        size="small"
        value={matcherType}
        onChange={setMatcherType}
        style={{ width: '100%' }}
      >
        <Option value="bash_glob">Bash glob (vd: git *, npm run *)</Option>
        <Option value="bash_regex">Bash regex</Option>
        <Option value="mcp_server">MCP server</Option>
        <Option value="mcp_glob">MCP glob (vd: mcp__memory__*)</Option>
        <Option value="tool_exact">Tool chính xác (Edit, Write…)</Option>
        <Option value="tool_category">Nhóm tool</Option>
        <Option value="always">Tất cả (always)</Option>
      </Select>

      {/* Pattern input */}
      {(matcherType === 'bash_glob' || matcherType === 'bash_regex' || matcherType === 'mcp_glob') && (
        <Input
          size="small"
          placeholder={matcherType === 'bash_glob' ? 'vd: git *, npm run *' : matcherType === 'bash_regex' ? 'vd: ^docker\\s+(rm|rmi)' : 'vd: mcp__memory__*'}
          value={pattern}
          onChange={e => setPattern(e.target.value)}
        />
      )}

      {matcherType === 'tool_exact' && (
        <Input size="small" placeholder="vd: Edit, Write, NotebookEdit" value={toolName} onChange={e => setToolName(e.target.value)} />
      )}

      {matcherType === 'mcp_server' && (
        <Space.Compact style={{ width: '100%' }}>
          <Input size="small" placeholder="Server (vd: memory)" value={mcpServer} onChange={e => setMcpServer(e.target.value)} style={{ width: '55%' }} />
          <Input size="small" placeholder="Tool (để trống = tất cả)" value={mcpTool} onChange={e => setMcpTool(e.target.value)} style={{ width: '45%' }} />
        </Space.Compact>
      )}

      {matcherType === 'tool_category' && (
        <Select size="small" value={category} onChange={setCategory} style={{ width: '100%' }}>
          <Option value="bash">Bash</Option>
          <Option value="file_edit">File Edit</Option>
          <Option value="skill">Skill</Option>
          <Option value="mcp">MCP</Option>
          <Option value="all">Tất cả</Option>
        </Select>
      )}

      {/* Action */}
      <Text style={{ fontSize: 11, fontWeight: 600, color: token.colorTextSecondary }}>Hành động</Text>
      <Select size="small" value={action} onChange={setAction} style={{ width: '100%' }}>
        <Option value="auto_accept">✅ Auto Accept — tự động chấp nhận</Option>
        <Option value="auto_accept_and_allow">💾 Accept + Save — lưu vào allowlist</Option>
        <Option value="force_request">❓ Force Ask — luôn hỏi dù skip bật</Option>
        <Option value="auto_deny">🚫 Auto Deny — tự động từ chối</Option>
      </Select>

      {/* Description */}
      <Input
        size="small"
        placeholder="Mô tả (tùy chọn)"
        value={description}
        onChange={e => setDescription(e.target.value)}
      />

      <Space>
        <Button size="small" type="primary" disabled={!isValid()} onClick={handleAdd} icon={<PlusOutlined />}>
          Thêm
        </Button>
        <Button size="small" onClick={onCancel} icon={<CloseOutlined />}>
          Hủy
        </Button>
      </Space>
    </div>
  );
}

// ─── Rule Row ────────────────────────────────────────────────────────────────

interface RuleRowProps {
  rule: ToolAutoAcceptRule;
  onToggle: (id: string) => void;
  onRemove: (id: string) => void;
  token: any;
}

function RuleRow({ rule, onToggle, onRemove, token }: RuleRowProps) {
  const ac = ACTION_CONFIG[rule.action];
  return (
    <div style={{
      display: 'flex',
      alignItems: 'center',
      gap: 6,
      padding: '6px 8px',
      borderRadius: 6,
      background: rule.enabled ? token.colorBgContainer : token.colorFillAlter,
      border: `1px solid ${token.colorBorderSecondary}`,
      opacity: rule.enabled ? 1 : 0.55,
    }}>
      {/* Icon */}
      <span style={{ color: token.colorTextTertiary, fontSize: 12, flexShrink: 0 }}>
        {ruleIcon(rule.matcher.type)}
      </span>

      {/* Label + description */}
      <div style={{ flex: 1, minWidth: 0 }}>
        <Tooltip title={rule.description ?? undefined} placement="left">
          <Text style={{ fontSize: 11, display: 'block', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
            {ruleLabel(rule)}
          </Text>
        </Tooltip>
      </div>

      {/* Action badge */}
      <Tag color={ac.color} style={{ margin: 0, fontSize: 10, padding: '0 4px', flexShrink: 0 }}>
        {ac.label}
      </Tag>

      {/* Toggle */}
      <Switch
        size="small"
        checked={rule.enabled}
        onChange={() => onToggle(rule.id)}
        style={{ flexShrink: 0 }}
      />

      {/* Delete */}
      <Popconfirm
        title="Xóa rule này?"
        onConfirm={() => onRemove(rule.id)}
        okText="Xóa"
        cancelText="Hủy"
        placement="left"
      >
        <Button
          type="text"
          size="small"
          danger
          icon={<DeleteOutlined />}
          style={{ padding: '0 2px', fontSize: 11, flexShrink: 0 }}
        />
      </Popconfirm>
    </div>
  );
}

// ─── Main Panel ──────────────────────────────────────────────────────────────

export interface ToolRulesPanelProps {
  rules: ToolAutoAcceptRule[];
  dangerouslyAcceptAll: boolean;
  onAddRule: (rule: ToolAutoAcceptRule) => void;
  onRemoveRule: (id: string) => void;
  onToggleRule: (id: string) => void;
  onToggleAcceptAll: (enabled: boolean) => void;
  /** Ẩn tiêu đề phụ "Tool Rules" (dùng trong Settings khi đã có Title trang) */
  embedded?: boolean;
}

export function ToolRulesPanel({
  rules,
  dangerouslyAcceptAll,
  onAddRule,
  onRemoveRule,
  onToggleRule,
  onToggleAcceptAll,
  embedded = false,
}: ToolRulesPanelProps) {
  const { token } = theme.useToken();
  const [showForm, setShowForm] = useState(false);

  const handleAdd = useCallback((rule: ToolAutoAcceptRule) => {
    onAddRule(rule);
    setShowForm(false);
  }, [onAddRule]);

  const enabledCount  = rules.filter(r => r.enabled).length;
  const forceAskCount = rules.filter(r => r.enabled && r.action === 'force_request').length;

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>

      {/* Section header */}
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
        <Space size={4}>
          {!embedded && (
            <Text style={{ fontSize: 11, textTransform: 'uppercase', color: token.colorTextDescription, letterSpacing: '0.5px' }}>
              Tool Rules
            </Text>
          )}
          {enabledCount > 0 && (
            <Tag style={{ margin: 0, fontSize: 10, padding: '0 4px' }}>
              {enabledCount} bật
            </Tag>
          )}
          {forceAskCount > 0 && (
            <Tag color="warning" style={{ margin: 0, fontSize: 10, padding: '0 4px' }}>
              {forceAskCount} audit
            </Tag>
          )}
        </Space>
        {!showForm && (
          <Button
            type="text"
            size="small"
            icon={<PlusOutlined />}
            onClick={() => setShowForm(true)}
            style={{ fontSize: 11, padding: '0 4px', color: token.colorPrimary }}
          >
            Thêm
          </Button>
        )}
      </div>

      {/* Add form */}
      {showForm && (
        <AddRuleForm
          onAdd={handleAdd}
          onCancel={() => setShowForm(false)}
          token={token}
        />
      )}

      {/* Rule list */}
      {rules.length > 0 ? (
        <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
          {rules.map(rule => (
            <RuleRow
              key={rule.id}
              rule={rule}
              onToggle={onToggleRule}
              onRemove={onRemoveRule}
              token={token}
            />
          ))}
        </div>
      ) : (
        !showForm && (
          <Text style={{ fontSize: 11, color: token.colorTextDescription, textAlign: 'center', padding: '8px 0' }}>
            Chưa có rule nào. Nhấn Thêm để tạo.
          </Text>
        )
      )}

      {/* Divider + Danger zone */}
      <Divider style={{ margin: '4px 0' }} />

      <div style={{
        borderRadius: 6,
        border: dangerouslyAcceptAll ? `1px solid ${token.colorError}` : `1px solid ${token.colorBorderSecondary}`,
        padding: '8px 10px',
        background: dangerouslyAcceptAll ? token.colorErrorBg : token.colorFillAlter,
        transition: 'all 0.2s',
      }}>
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 8 }}>
          <Space size={4}>
            <WarningOutlined style={{ color: token.colorError, fontSize: 12 }} />
            <Text style={{ fontSize: 11, fontWeight: 600, color: token.colorError }}>
              Chấp nhận tất cả
            </Text>
          </Space>
          <Switch
            size="small"
            checked={dangerouslyAcceptAll}
            onChange={onToggleAcceptAll}
          />
        </div>
        {dangerouslyAcceptAll && (
          <Alert
            type="error"
            showIcon={false}
            message={
              <Text style={{ fontSize: 10, color: token.colorErrorText }}>
                Mọi tool request đều được tự động chấp nhận — không có xác nhận. Chỉ dùng cho môi trường an toàn.
              </Text>
            }
            style={{ marginTop: 6, padding: '4px 8px', borderRadius: 4 }}
          />
        )}
      </div>
    </div>
  );
}
