// Structured trigger editor for cowork team members.
//
// Mirrors the trigger schema from the legacy CoworkManager
// (`src/cowork/mod.rs::collect_triggered_tasks` in git history): a JSON
// array of typed rules. Each rule has a `type` plus type-specific fields:
//
//   message_received    — { type, from?, messageType? }
//   on_mention          — { type, from }
//   task_assigned       — { type }
//   task_status_changed — { type, status?, assignee?, to? }
//   cron                — { type, cron }   (lightweight extension)
//
// Triggers are stored as a JSON-string blob on the member so the backend
// schema doesn't change. The blob is parsed on edit-modal open and
// re-serialized on save. Invalid JSON falls back to an empty list.

import { Button, Input, Select, theme, Tag } from 'antd';
import { CloseOutlined, PlusOutlined } from '@ant-design/icons';

export type TriggerRule =
  | { type: 'message_received'; from?: string; messageType?: string }
  | { type: 'on_mention'; from?: string }
  | { type: 'task_assigned' }
  | { type: 'task_status_changed'; status?: string; assignee?: string; to?: string }
  | { type: 'cron'; cron?: string };

const TRIGGER_TYPES: TriggerRule['type'][] = [
  'message_received',
  'on_mention',
  'task_assigned',
  'task_status_changed',
  'cron',
];

const LABELS: Record<TriggerRule['type'], string> = {
  message_received: '💬 Message received',
  on_mention: '@ Mention',
  task_assigned: '📌 Task assigned',
  task_status_changed: '🔁 Task status changed',
  cron: '⏰ Cron schedule',
};

export function parseTriggerJson(raw: string | undefined | null): TriggerRule[] {
  if (!raw) return [];
  try {
    const v = JSON.parse(raw);
    if (Array.isArray(v)) return v.filter(x => x && typeof x === 'object' && typeof x.type === 'string') as TriggerRule[];
  } catch {}
  return [];
}

export function stringifyTriggers(rules: TriggerRule[]): string | null {
  if (rules.length === 0) return null;
  return JSON.stringify(rules);
}

/** Short summary for tags — e.g. "2 rules" or "msg + cron". */
export function summarizeTriggers(raw: string | undefined | null): string {
  const rules = parseTriggerJson(raw);
  if (rules.length === 0) return '';
  if (rules.length === 1) return LABELS[rules[0].type].replace(/^.\s/, '');
  return `${rules.length} rules`;
}

interface Props {
  value?: TriggerRule[];
  onChange?: (rules: TriggerRule[]) => void;
}

export function TriggerEditor({ value = [], onChange }: Props) {
  const { token } = theme.useToken();

  const update = (idx: number, patch: Partial<TriggerRule>) => {
    const next = [...value];
    next[idx] = { ...next[idx], ...patch } as TriggerRule;
    onChange?.(next);
  };

  const setType = (idx: number, type: TriggerRule['type']) => {
    const next = [...value];
    next[idx] = { type } as TriggerRule;
    onChange?.(next);
  };

  const remove = (idx: number) => {
    const next = value.filter((_, i) => i !== idx);
    onChange?.(next);
  };

  const add = () => {
    onChange?.([...value, { type: 'message_received' }]);
  };

  return (
    <div className="space-y-2">
      {value.length === 0 && (
        <div
          className="text-xs px-3 py-2 rounded-md"
          style={{ background: token.colorFillAlter, color: token.colorTextTertiary }}
        >
          No triggers — this member only activates when manually dispatched.
        </div>
      )}

      {value.map((rule, idx) => (
        <div
          key={idx}
          className="rounded-md p-2 space-y-2"
          style={{ border: `1px solid ${token.colorBorderSecondary}`, background: token.colorBgContainer }}
        >
          <div className="flex items-center gap-2">
            <Tag style={{ marginRight: 0, fontSize: 10 }}>#{idx + 1}</Tag>
            <Select
              size="small"
              value={rule.type}
              onChange={(v) => setType(idx, v)}
              options={TRIGGER_TYPES.map(t => ({ value: t, label: LABELS[t] }))}
              style={{ flex: 1 }}
            />
            <Button
              type="text"
              size="small"
              danger
              icon={<CloseOutlined />}
              onClick={() => remove(idx)}
              aria-label="Remove trigger"
            />
          </div>

          {rule.type === 'message_received' && (
            <div className="grid grid-cols-2 gap-2">
              <Input
                size="small"
                placeholder="from (member id or 'user')"
                value={(rule as any).from ?? ''}
                onChange={(e) => update(idx, { from: e.target.value || undefined } as any)}
              />
              <Input
                size="small"
                placeholder="messageType (e.g. handoff)"
                value={(rule as any).messageType ?? ''}
                onChange={(e) => update(idx, { messageType: e.target.value || undefined } as any)}
              />
            </div>
          )}

          {rule.type === 'on_mention' && (
            <Input
              size="small"
              placeholder="from (member id who mentioned)"
              value={(rule as any).from ?? ''}
              onChange={(e) => update(idx, { from: e.target.value || undefined } as any)}
            />
          )}

          {rule.type === 'task_status_changed' && (
            <div className="grid grid-cols-3 gap-2">
              <Input
                size="small"
                placeholder="status (done/blocked)"
                value={(rule as any).status ?? ''}
                onChange={(e) => update(idx, { status: e.target.value || undefined } as any)}
              />
              <Input
                size="small"
                placeholder="assignee (member id)"
                value={(rule as any).assignee ?? ''}
                onChange={(e) => update(idx, { assignee: e.target.value || undefined } as any)}
              />
              <Input
                size="small"
                placeholder="to (handoff target)"
                value={(rule as any).to ?? ''}
                onChange={(e) => update(idx, { to: e.target.value || undefined } as any)}
              />
            </div>
          )}

          {rule.type === 'cron' && (
            <Input
              size="small"
              placeholder="cron expression (e.g. 0 9 * * 1)"
              style={{ fontFamily: 'monospace' }}
              value={(rule as any).cron ?? ''}
              onChange={(e) => update(idx, { cron: e.target.value || undefined } as any)}
            />
          )}

          {rule.type === 'task_assigned' && (
            <div className="text-[11px]" style={{ color: token.colorTextTertiary }}>
              Fires whenever a task is assigned to this member. No extra fields.
            </div>
          )}
        </div>
      ))}

      <Button
        type="dashed"
        size="small"
        icon={<PlusOutlined />}
        onClick={add}
        block
      >
        Add trigger rule
      </Button>
    </div>
  );
}
