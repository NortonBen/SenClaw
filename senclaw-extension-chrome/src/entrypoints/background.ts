// Background service worker: WebSocket client + message routing.
import { WSClient } from '../lib/ws-client';
import type { ConnectionState } from '../lib/ws-client';
import { MessageBridge } from '../lib/message-bridge';
import { TabsController } from '../agent/TabsController';
import { TabGroupController } from '../agent/TabGroupController';
import { SearchEngine } from '../agent/SearchEngine';
import { CrawlEngine } from '../agent/CrawlEngine';
import { getWsHost, getWsPort } from '../lib/storage';
import type { DaemonMessage, AgentId, TabId } from '../types/protocol';

const MAX_LOGS = 200;

export type LogLevel = 'info' | 'event' | 'warn' | 'error';
export interface LogEntry {
  ts: string;          // ISO timestamp — UI formats it
  level: LogLevel;
  message: string;
}

const activityLogs: LogEntry[] = [];

function pushLog(level: LogLevel, message: string): void {
  const entry: LogEntry = {
    ts: new Date().toISOString(),
    level,
    message,
  };
  activityLogs.push(entry);
  if (activityLogs.length > MAX_LOGS) activityLogs.shift();
  chrome.runtime
    .sendMessage({ type: 'activity-log', entry })
    .catch(() => {
      // No listener (panel closed). Expected.
    });
}

const log = {
  info:  (m: string) => pushLog('info',  m),
  event: (m: string) => pushLog('event', m),
  warn:  (m: string) => pushLog('warn',  m),
  error: (m: string) => pushLog('error', m),
};

function shortDetail(s: string | undefined, max = 80): string {
  if (!s) return '';
  return s.length > max ? `${s.slice(0, max - 1)}…` : s;
}

export default defineBackground(() => {
  setupBackground();
});

async function setupBackground() {
  const [wsHost, wsPort] = await Promise.all([getWsHost(), getWsPort()]);
  const ws = new WSClient(wsHost, wsPort);
  const groupController = new TabGroupController();
  const tabs = new TabsController(groupController);
  const searcher = new SearchEngine();
  const crawler = new CrawlEngine();
  let connectionState: ConnectionState = 'idle';

  groupController.setupListeners();

  // Reflect WS state into logs for visibility.
  ws.onStatusChange((state, detail) => {
    connectionState = state;
    const where = ws.getEndpoint();
    const tail = detail ? ` — ${shortDetail(detail, 120)}` : '';
    switch (state) {
      case 'connecting':
        log.info(`Connecting to ${where}${tail}`);
        break;
      case 'connected':
        log.event(`Connected to ${where}`);
        break;
      case 'reconnecting':
        log.warn(`Reconnecting${tail}`);
        break;
      case 'disconnected':
        log.warn(`Disconnected${tail}`);
        break;
      case 'idle':
        break;
    }
    // Broadcast a UI-friendly status update too (cheap).
    chrome.runtime
      .sendMessage({ type: 'connection-state', state, detail })
      .catch(() => { /* no listener */ });
  });

  // ===== Tab lifecycle events -> Daemon (ONLY for SenClaw tabs) =====
  tabs.onTabCreated((tab) => {
    if (!tab.isSenclawTab) return;
    log.info(`Tab created (agent=${tab.agentId ?? 'default'}): ${shortDetail(tab.url)}`);
    ws.send({
      type: 'TabCreated',
      tab_id: tab.id,
      agent_id: tab.agentId ?? DEFAULT_AGENT_ID,
      url: tab.url,
      window_id: tab.windowId,
      group_id: tab.groupId,
    });
  });

  tabs.onTabUpdated((tab) => {
    if (!tab.isSenclawTab) return;
    ws.setActiveTabId(tab.id);
    ws.send({
      type: 'TabUpdated',
      tab_id: tab.id,
      agent_id: tab.agentId ?? DEFAULT_AGENT_ID,
      url: tab.url,
      title: tab.title,
      status: tab.status,
      group_id: tab.groupId,
    });
  });

  tabs.onTabClosed((tab) => {
    if (!tab.isSenclawTab) return;
    log.info(`Tab closed (agent=${tab.agentId ?? 'default'}, id=${tab.id})`);
    ws.send({
      type: 'TabClosed',
      tab_id: tab.id,
      agent_id: tab.agentId ?? DEFAULT_AGENT_ID,
    });
  });

  // ===== Crawl events -> Daemon =====
  crawler.setProgressCallback((jobId, pagesCrawled, pagesTotal, currentUrl, agentId?) => {
    ws.send({
      type: 'CrawlProgress',
      job_id: jobId,
      agent_id: agentId ?? 'unknown',
      pages_crawled: pagesCrawled,
      pages_total: pagesTotal,
      current_url: currentUrl,
    });
  });

  crawler.setResultCallback((jobId, pageResult, agentId?) => {
    ws.send({
      type: 'CrawlResult',
      job_id: jobId,
      agent_id: agentId ?? 'unknown',
      page_result: pageResult,
    });
  });

  crawler.setCompleteCallback((jobId, totalPages, durationMs, agentId?) => {
    log.event(`Crawl complete (job=${jobId.slice(0, 8)}): ${totalPages} page(s) in ${durationMs}ms`);
    ws.send({
      type: 'CrawlComplete',
      job_id: jobId,
      agent_id: agentId ?? 'unknown',
      total_pages: totalPages,
      duration_ms: durationMs,
    });
  });

  // ===== Handle messages from Daemon =====
  ws.onMessage(async (msg: DaemonMessage) => {
    try {
      logIncoming(msg);
      await handleDaemonMessage(msg, tabs, searcher, crawler, ws);
    } catch (e: unknown) {
      const errMsg = e instanceof Error ? e.message : String(e);
      log.error(`Handler error for ${msg.type}: ${shortDetail(errMsg, 160)}`);
      console.error('[SenClaw] Error handling message:', msg.type, errMsg);
      if ('request_id' in msg) {
        const agentId = 'agent_id' in msg ? (msg as any).agent_id : undefined;
        ws.send({
          type: 'Response',
          request_id: (msg as any).request_id,
          agent_id: agentId,
          status: 'error',
          message: errMsg,
        });
      }
    }
  });

  ws.connect();

  // ===== Keep the MV3 service worker connection resilient =====
  // Chrome suspends idle service workers (~30s). The 15s heartbeat keeps the
  // worker alive while the daemon WS is up; if the worker WAS suspended, a
  // periodic alarm wakes it so we can re-establish the connection.
  // ws.connect() is a no-op when the socket is already OPEN/CONNECTING.
  const KEEPALIVE_ALARM = 'senclaw-keepalive';
  chrome.alarms.create(KEEPALIVE_ALARM, { periodInMinutes: 0.4 });
  chrome.alarms.onAlarm.addListener((alarm) => {
    if (alarm.name !== KEEPALIVE_ALARM) return;
    // Only log the tick if we're not in a steady-state connected — reduces noise.
    if (connectionState !== 'connected') {
      log.info(`Keepalive tick (state=${connectionState})`);
    }
    ws.connect();
  });
  chrome.runtime.onStartup.addListener(() => ws.connect());
  chrome.runtime.onInstalled.addListener(() => ws.connect());

  // Held port = defer suspension while side panel (or any view) is open.
  chrome.runtime.onConnect.addListener(() => {});
  chrome.sidePanel?.setPanelBehavior?.({ openPanelOnActionClick: true }).catch(() => {});

  // React to settings changes from the side panel: rebind WS endpoint.
  chrome.storage.onChanged.addListener(async (changes, area) => {
    if (area !== 'local') return;
    if (!changes.ws_host && !changes.ws_port) return;
    const [host, port] = await Promise.all([getWsHost(), getWsPort()]);
    log.info(`Endpoint updated: ws://${host}:${port}/browser — reconnecting`);
    ws.setEndpoint(host, port);
  });

  // ===== Side-panel ↔ background RPC =====
  chrome.runtime.onMessage.addListener((m, _sender, sendResponse) => {
    if (!m || typeof m.type !== 'string') return false;
    switch (m.type) {
      case 'get-connection-status':
        sendResponse({
          state: connectionState,
          connected: connectionState === 'connected',
          endpoint: ws.getEndpoint(),
        });
        return false;
      case 'get-activity-logs':
        sendResponse({ logs: activityLogs.slice() });
        return false;
      case 'clear-activity-logs':
        activityLogs.length = 0;
        chrome.runtime
          .sendMessage({ type: 'activity-logs-cleared' })
          .catch(() => { /* no listener */ });
        sendResponse({ ok: true });
        return false;
      case 'reconnect-now':
        log.info('Manual reconnect requested');
        ws.connect();
        sendResponse({ ok: true });
        return false;
    }
    return false;
  });
}

function logIncoming(msg: DaemonMessage): void {
  const t = msg.type;
  const agent =
    'agent_id' in msg && (msg as any).agent_id
      ? ` [agent ${(msg as any).agent_id}]`
      : '';
  // Hand-pick the most useful detail per type.
  let detail = '';
  switch (t) {
    case 'Navigate':
      detail = `→ ${shortDetail((msg as any).url)}`;
      break;
    case 'NewTab':
      detail = (msg as any).url ? `→ ${shortDetail((msg as any).url)}` : '';
      break;
    case 'CloseTab':
    case 'SwitchTab':
    case 'GoBack':
    case 'GoForward':
    case 'Reload':
      detail = (msg as any).tab_id ? `tab=${(msg as any).tab_id}` : '';
      break;
    case 'Search':
      detail = `"${shortDetail((msg as any).query, 50)}" on ${(msg as any).engine}`;
      break;
    case 'CrawlStart':
      detail = `${shortDetail((msg as any).start_url)} depth=${(msg as any).depth}`;
      break;
    case 'Click':
    case 'Type':
    case 'Hover':
    case 'Scroll':
    case 'PressKey':
    case 'SelectOption':
      detail = (msg as any).index !== undefined ? `index=${(msg as any).index}` : '';
      break;
    case 'GetSnapshot':
    case 'GetScreenshot':
    case 'ExtractText':
    case 'ExtractLinks':
    case 'ExtractTable':
      detail = '';
      break;
  }
  pushLog('event', `← ${t}${agent}${detail ? ` ${detail}` : ''}`);
}

// Default agent ID when agent_id is not provided (backward compatibility)
const DEFAULT_AGENT_ID: AgentId = 'default-agent';

async function handleDaemonMessage(
  msg: DaemonMessage,
  tabs: TabsController,
  searcher: SearchEngine,
  crawler: CrawlEngine,
  ws: WSClient,
): Promise<void> {
  // Get agent_id from message, fallback to default for backward compatibility
  const agentId: AgentId = ('agent_id' in msg && (msg as any).agent_id)
    ? (msg as any).agent_id as AgentId
    : DEFAULT_AGENT_ID;

  switch (msg.type) {
    // ===== Tab Management (per-agent) =====
    case 'Navigate': {
      const active = msg.active === true; // default false — background like Claude
      const tab = await tabs.navigateForAgent(agentId, msg.url, undefined, active);
      ws.send({
        type: 'Response',
        request_id: msg.request_id,
        agent_id: agentId,
        status: 'ok',
        data: { tab_id: tab.id, url: tab.url, agent_id: agentId, active },
      });
      break;
    }

    case 'NewTab': {
      const activeNewTab = msg.active === true;
      const tab = await tabs.getOrCreateForAgent(agentId, msg.url, undefined, activeNewTab);
      ws.send({
        type: 'Response',
        request_id: msg.request_id,
        agent_id: agentId,
        status: 'ok',
        data: { tab_id: tab.id, agent_id: agentId, active: activeNewTab },
      });
      break;
    }

    case 'CloseTab': {
      await tabs.closeForAgent(agentId);
      ws.send({
        type: 'Response',
        request_id: msg.request_id,
        agent_id: agentId,
        status: 'ok',
        data: { agent_id: agentId },
      });
      break;
    }

    case 'SwitchTab': {
      if (!tabs.isSenclawTab(msg.tab_id)) {
        log.warn(`SwitchTab refused: tab ${msg.tab_id} not managed by SenClaw`);
        ws.send({ type: 'Response', request_id: msg.request_id, status: 'error', message: 'Tab not managed by SenClaw' });
        return;
      }
      await tabs.switchTo(msg.tab_id);
      ws.send({ type: 'Response', request_id: msg.request_id, agent_id: agentId, status: 'ok', data: {} });
      break;
    }

    case 'GoBack': {
      const targetTabId = msg.tab_id ?? (agentId ? tabs.groupController?.getAgentTabId(agentId) : null);
      if (!targetTabId) {
        log.warn('GoBack: no tab specified');
        ws.send({ type: 'Response', request_id: msg.request_id, status: 'error', message: 'No tab specified' });
        return;
      }
      await tabs.goBack(targetTabId);
      ws.send({ type: 'Response', request_id: msg.request_id, agent_id: agentId, status: 'ok', data: {} });
      break;
    }

    case 'GoForward': {
      const targetTabId = msg.tab_id ?? (agentId ? tabs.groupController?.getAgentTabId(agentId) : null);
      if (!targetTabId) {
        log.warn('GoForward: no tab specified');
        ws.send({ type: 'Response', request_id: msg.request_id, status: 'error', message: 'No tab specified' });
        return;
      }
      await tabs.goForward(targetTabId);
      ws.send({ type: 'Response', request_id: msg.request_id, agent_id: agentId, status: 'ok', data: {} });
      break;
    }

    case 'Reload': {
      const targetTabId = msg.tab_id ?? (agentId ? tabs.groupController?.getAgentTabId(agentId) : null);
      if (!targetTabId) {
        log.warn('Reload: no tab specified');
        ws.send({ type: 'Response', request_id: msg.request_id, status: 'error', message: 'No tab specified' });
        return;
      }
      await tabs.reload(targetTabId);
      ws.send({ type: 'Response', request_id: msg.request_id, agent_id: agentId, status: 'ok', data: {} });
      break;
    }

    case 'ListTabs': {
      const tabList = tabs.listTabsForAgent(agentId);
      ws.send({
        type: 'Response',
        request_id: msg.request_id,
        agent_id: agentId,
        status: 'ok',
        data: {
          tabs: tabList,
          agent_mappings: tabs.groupController?.getAgentTabMappings(),
        },
      });
      break;
    }

    // ===== DOM Interaction -> Content Script =====
    case 'Click':
    case 'Type':
    case 'SelectOption':
    case 'Scroll':
    case 'Hover':
    case 'PressKey':
    case 'UploadFile': {
      const targetTabId = msg.tab_id ?? tabs.groupController?.getAgentTabId(agentId) ?? tabs.getActiveTabId() ?? undefined;
      if (!targetTabId) {
        log.warn(`${msg.type}: no tab available for agent ${agentId}`);
        ws.send({ type: 'Response', request_id: msg.request_id, status: 'error', message: 'No tab available' });
        return;
      }
      const result = await MessageBridge.sendToTab(targetTabId, msg);
      if (result.status === 'error') {
        log.error(`${msg.type} failed: ${shortDetail((result as any).message, 120)}`);
      }
      ws.send({ type: 'Response', request_id: msg.request_id, agent_id: agentId, ...result });
      break;
    }

    // ===== Execute JS =====
    case 'ExecuteJs': {
      const targetTabId = msg.tab_id ?? tabs.groupController?.getAgentTabId(agentId) ?? tabs.getActiveTabId() ?? undefined;
      if (!targetTabId) {
        log.warn('ExecuteJs: no tab available');
        ws.send({ type: 'Response', request_id: msg.request_id, status: 'error', message: 'No tab available' });
        return;
      }
      const result = await MessageBridge.sendToTab(targetTabId, {
        type: 'ExecuteJs',
        script: msg.script,
      });
      if (result.status === 'error') {
        log.error(`ExecuteJs failed: ${shortDetail((result as any).message, 120)}`);
      }
      ws.send({ type: 'Response', request_id: msg.request_id, agent_id: agentId, ...result });
      break;
    }

    // ===== Wait =====
    case 'WaitFor': {
      const condition = msg.condition;
      if (condition.type === 'time') {
        await new Promise(r => setTimeout(r, condition.ms));
        ws.send({ type: 'Response', request_id: msg.request_id, agent_id: agentId, status: 'ok', data: {} });
      } else if (condition.type === 'navigation') {
        const targetTabId = msg.tab_id ?? tabs.groupController?.getAgentTabId(agentId);
        if (targetTabId) {
          await new Promise<void>((resolve) => {
            const timeout = setTimeout(resolve, condition.timeout_ms);
            const listener = (_tabId: number, changeInfo: chrome.tabs.TabChangeInfo) => {
              if (_tabId === parseInt(targetTabId) && changeInfo.status === 'complete') {
                clearTimeout(timeout);
                chrome.tabs.onUpdated.removeListener(listener);
                resolve();
              }
            };
            chrome.tabs.onUpdated.addListener(listener);
          });
        }
        ws.send({ type: 'Response', request_id: msg.request_id, agent_id: agentId, status: 'ok', data: {} });
      } else {
        ws.send({ type: 'Response', request_id: msg.request_id, agent_id: agentId, status: 'ok', data: { message: 'Wait condition handled by content script' } });
      }
      break;
    }

    // ===== Observation =====
    case 'GetSnapshot':
    case 'ExtractText':
    case 'ExtractLinks':
    case 'ExtractTable':
    case 'GetScreenshot': {
      const payload: Record<string, unknown> = { type: msg.type };
      if ('depth' in msg) (payload as any).depth = msg.depth;
      if ('compress_html' in msg) (payload as any).compress_html = msg.compress_html;
      if ('selector' in msg) (payload as any).selector = msg.selector;
      if ('full_page' in msg) (payload as any).full_page = msg.full_page;
      if ('format' in msg) (payload as any).format = msg.format;
      if ('quality' in msg) (payload as any).quality = msg.quality;

      const targetTabId = msg.tab_id ?? tabs.groupController?.getAgentTabId(agentId) ?? tabs.getActiveTabId() ?? undefined;
      if (!targetTabId) {
        log.warn(`${msg.type}: no tab available`);
        ws.send({ type: 'Response', request_id: msg.request_id, status: 'error', message: 'No tab available' });
        return;
      }

      const result = await MessageBridge.sendToTab(targetTabId, payload as any);
      if (result.status === 'error') {
        log.error(`${msg.type} failed: ${shortDetail((result as any).message, 120)}`);
      }
      ws.send({ type: 'Response', request_id: msg.request_id, agent_id: agentId, ...result });
      break;
    }

    // ===== Search (creates/uses agent's tab) =====
    case 'Search': {
      const activeSearch = msg.active === true;
      const tab = await tabs.getOrCreateForAgent(agentId, undefined, activeSearch);
      await new Promise(r => setTimeout(r, 500));

      try {
        const results = await searcher.search(tab.id, msg.query, msg.engine, msg.num_results, msg.language);
        log.event(`Search complete: ${results.results.length} result(s) for "${shortDetail(msg.query, 50)}"`);
        ws.send({
          type: 'Response',
          request_id: msg.request_id,
          agent_id: agentId,
          status: 'ok',
          data: { ...results, agent_id: agentId, tab_id: tab.id, active: activeSearch },
        });
      } catch (e: unknown) {
        const errMsg = e instanceof Error ? e.message : String(e);
        log.error(`Search failed: ${shortDetail(errMsg, 120)}`);
        ws.send({
          type: 'Response',
          request_id: msg.request_id,
          agent_id: agentId,
          status: 'error',
          message: errMsg,
        });
      }
      break;
    }

    // ===== Crawl (uses agent's tab) =====
    case 'CrawlStart': {
      const activeCrawl = msg.active === true;
      const tab = await tabs.getOrCreateForAgent(agentId, msg.start_url, activeCrawl);

      crawler.start({
        job_id: msg.job_id,
        agent_id: agentId,
        tab_id: tab.id,
        start_url: msg.start_url,
        depth: msg.depth,
        max_pages: msg.max_pages,
        link_patterns: msg.link_patterns,
        exclude_patterns: msg.exclude_patterns,
        same_domain: msg.same_domain,
        per_page_timeout_ms: 10000,
        wait_between_pages_ms: 1000,
      });
      ws.send({
        type: 'Response',
        request_id: msg.job_id,
        agent_id: agentId,
        status: 'ok',
        data: { job_id: msg.job_id, status: 'started', agent_id: agentId, tab_id: tab.id, active: activeCrawl },
      });
      break;
    }
    case 'CrawlStop': {
      crawler.stop(msg.job_id);
      log.info(`Crawl stopped (job=${msg.job_id.slice(0, 8)})`);
      break;
    }
    case 'CrawlPause': {
      crawler.pause(msg.job_id);
      log.info(`Crawl paused (job=${msg.job_id.slice(0, 8)})`);
      break;
    }
    case 'CrawlResume': {
      crawler.resume(msg.job_id);
      log.info(`Crawl resumed (job=${msg.job_id.slice(0, 8)})`);
      break;
    }

    // ===== Fill Form =====
    case 'FillForm': {
      const targetTabId = msg.tab_id ?? tabs.groupController?.getAgentTabId(agentId) ?? tabs.getActiveTabId() ?? undefined;
      if (!targetTabId) {
        log.warn('FillForm: no tab available');
        ws.send({ type: 'Response', request_id: msg.request_id, status: 'error', message: 'No tab available' });
        return;
      }
      const result = await MessageBridge.sendToTab(targetTabId, msg);
      if (result.status === 'error') {
        log.error(`FillForm failed: ${shortDetail((result as any).message, 120)}`);
      }
      ws.send({ type: 'Response', request_id: msg.request_id, agent_id: agentId, ...result });
      break;
    }

    // ===== Status =====
    case 'GetStatus': {
      const [activeTab] = await chrome.tabs.query({ active: true, currentWindow: true });
      const senclawTabs = tabs.listSenclawTabs();
      const agentTabInfo = tabs.listTabsForAgent(agentId);

      ws.send({
        type: 'Response',
        request_id: msg.request_id,
        agent_id: agentId,
        status: 'ok',
        data: {
          connected: true,
          senclaw_tab_count: senclawTabs.length,
          senclaw_tabs: senclawTabs,
          agent_tab: agentTabInfo?.[0] ?? null,
          agent_mappings: tabs.groupController?.getAgentTabMappings(),
          active_tab_id: activeTab?.id?.toString() ?? null,
          active_is_senclaw: activeTab?.id ? tabs.isSenclawTab(activeTab.id.toString()) : false,
        },
      });
      break;
    }

    default:
      log.warn(`Unknown message type: ${(msg as any).type}`);
      console.warn('[SenClaw] Unknown message type:', (msg as any).type);
  }
}
