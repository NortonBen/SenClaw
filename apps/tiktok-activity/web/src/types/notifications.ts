export type NotificationEvent = "run_failed" | "run_done" | "flow_action";

export interface Notification {
  id: string;
  ruleId: string;
  event: NotificationEvent;
  title: string;
  body: string;
  runId: string;
  accountId: string;
  flowId: string;
  readAt?: string;
  createdAt?: string;
}

export interface NotificationRule {
  id: string;
  name: string;
  enabled: boolean;
  event: NotificationEvent;
  flowId: string;
  accountId: string;
  messageTemplate: string;
  createdAt?: string;
  updatedAt?: string;
}

