// Background service worker: WebSocket client + message routing.
import { WSClient } from '../lib/ws-client';
import { MessageBridge } from '../lib/message-bridge';
import { TabsController } from '../agent/TabsController';
import { TabGroupController } from '../agent/TabGroupController';
import { SearchEngine } from '../agent/SearchEngine';
import { CrawlEngine } from '../agent/CrawlEngine';
import { getWsPort } from '../lib/storage';
import type { DaemonMessage, AgentId, TabId } from '../types/protocol';

const MAX_LOGS = 50;
let activityLogs: string[] = [];

function logActivity(message: string) {
  const timestamp = new Date().toLocaleTimeString();
  const entry = `[${timestamp}] ${message}`;
  activityLogs.push(entry);
  if (activityLogs.length > MAX_LOGS) activityLogs.shift();

  // Broadcast to side panel or other parts of the extension
  chrome.runtime.sendMessage({ type: 'activity-log', entry }).catch(() => {
    // Expected to fail if no listener is active (side panel closed)
  });
}

export default defineBackground(() => {
  setupBackground();
});

async function setupBackground() {
  const wsPort = await getWsPort();
  const ws = new WSClient(wsPort);
  const groupController = new TabGroupController();
  const tabs = new TabsController(groupController);
  const searcher = new SearchEngine();
  const crawler = new CrawlEngine();

  // Setup group controller listeners
  groupController.setupListeners();

  // ===== Tab lifecycle events -> Daemon (ONLY for SenClaw tabs) =====
  tabs.onTabCreated((tab) => {
    // Only send events for tabs opened by SenClaw (in our group)
    if (!tab.isSenclawTab) return;

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
    // Only send events for tabs opened by SenClaw
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
    // Only send events for tabs opened by SenClaw
    if (!tab.isSenclawTab) return;

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
      logActivity(`Received: ${msg.type}`);
      await handleDaemonMessage(msg, tabs, searcher, crawler, ws);
    } catch (e: unknown) {
      const errMsg = e instanceof Error ? e.message : String(e);
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

  // Connect to daemon
  ws.connect();

  // Keep service worker alive
  chrome.runtime.onConnect.addListener(() => {});
  chrome.sidePanel?.setPanelBehavior?.({ openPanelOnActionClick: true }).catch(() => {});

  // Respond to status/log queries from side panel
  let connected = false;
  ws.onStatusChange((status) => { connected = status; });
  chrome.runtime.onMessage.addListener((_msg, _sender, sendResponse) => {
    if (_msg?.type === 'get-connection-status') {
      sendResponse({ connected });
    } else if (_msg?.type === 'get-activity-logs') {
      sendResponse({ logs: activityLogs });
    } else if (_msg?.type === 'send-chat') {
      ws.send({ type: 'UserInstruction', text: _msg.text });
      sendResponse({ status: 'ok' });
    }
  });
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
      logActivity(`[Agent ${agentId}] Navigating to: ${msg.url} (active: ${active})`);
      // Navigate using agent's dedicated tab (creates or reuses)
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
      const activeNewTab = msg.active === true; // default false — background like Claude
      // Get or create tab for this agent (reuses existing if any)
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
      // Close the agent's tab
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
      // Switch to specified tab (must be SenClaw managed)
      if (!tabs.isSenclawTab(msg.tab_id)) {
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
        ws.send({ type: 'Response', request_id: msg.request_id, status: 'error', message: 'No tab specified' });
        return;
      }
      await tabs.reload(targetTabId);
      ws.send({ type: 'Response', request_id: msg.request_id, agent_id: agentId, status: 'ok', data: {} });
      break;
    }

    case 'ListTabs': {
      // List tabs for this agent (or all SenClaw tabs if filtering)
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
      logActivity(`[Agent ${agentId}] Action: ${msg.type}`);
      // Use agent's tab if no specific tab_id provided
      const targetTabId = msg.tab_id ?? tabs.groupController?.getAgentTabId(agentId) ?? tabs.getActiveTabId() ?? undefined;
      if (!targetTabId) {
        ws.send({ type: 'Response', request_id: msg.request_id, status: 'error', message: 'No tab available' });
        return;
      }
      const result = await MessageBridge.sendToTab(targetTabId, msg);
      ws.send({ type: 'Response', request_id: msg.request_id, agent_id: agentId, ...result });
      break;
    }

    // ===== Execute JS =====
    case 'ExecuteJs': {
      const targetTabId = msg.tab_id ?? tabs.groupController?.getAgentTabId(agentId) ?? tabs.getActiveTabId() ?? undefined;
      if (!targetTabId) {
        ws.send({ type: 'Response', request_id: msg.request_id, status: 'error', message: 'No tab available' });
        return;
      }
      const result = await MessageBridge.sendToTab(targetTabId, {
        type: 'ExecuteJs',
        script: msg.script,
      });
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
        ws.send({ type: 'Response', request_id: msg.request_id, status: 'error', message: 'No tab available' });
        return;
      }

      const result = await MessageBridge.sendToTab(targetTabId, payload as any);
      ws.send({ type: 'Response', request_id: msg.request_id, agent_id: agentId, ...result });
      break;
    }

    // ===== Search (creates/uses agent's tab) =====
    case 'Search': {
      const activeSearch = msg.active === true; // default false — background like Claude
      logActivity(`[Agent ${agentId}] Searching: "${msg.query}" on ${msg.engine} (active: ${activeSearch})`);

      // Get or create tab for this agent
      const tab = await tabs.getOrCreateForAgent(agentId, undefined, activeSearch);
      await new Promise(r => setTimeout(r, 500));

      const results = await searcher.search(tab.id, msg.query, msg.engine, msg.num_results, msg.language);
      ws.send({
        type: 'Response',
        request_id: msg.request_id,
        agent_id: agentId,
        status: 'ok',
        data: { ...results, agent_id: agentId, tab_id: tab.id, active: activeSearch },
      });
      logActivity(`[Agent ${agentId}] Search complete: ${results.results.length} results found`);
      // NOTE: We don't close the tab - user requested to keep all tabs open
      break;
    }

    // ===== Crawl (uses agent's tab) =====
    case 'CrawlStart': {
      const activeCrawl = msg.active === true; // default false — background like Claude
      logActivity(`[Agent ${agentId}] Starting crawl: ${msg.start_url} (active: ${activeCrawl})`);

      // Get or create tab for this agent
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
      break;
    }
    case 'CrawlPause': {
      crawler.pause(msg.job_id);
      break;
    }
    case 'CrawlResume': {
      crawler.resume(msg.job_id);
      break;
    }

    // ===== Fill Form =====
    case 'FillForm': {
      const targetTabId = msg.tab_id ?? tabs.groupController?.getAgentTabId(agentId) ?? tabs.getActiveTabId() ?? undefined;
      if (!targetTabId) {
        ws.send({ type: 'Response', request_id: msg.request_id, status: 'error', message: 'No tab available' });
        return;
      }
      const result = await MessageBridge.sendToTab(targetTabId, msg);
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
      console.warn('[SenClaw] Unknown message type:', (msg as any).type);
  }
}
