// Background service worker: WebSocket client + message routing.
import { WSClient } from '../lib/ws-client';
import { MessageBridge } from '../lib/message-bridge';
import { TabsController } from '../agent/TabsController';
import { SearchEngine } from '../agent/SearchEngine';
import { CrawlEngine } from '../agent/CrawlEngine';
import { getWsPort } from '../lib/storage';
import type { DaemonMessage } from '../types/protocol';

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
  const tabs = new TabsController();
  const searcher = new SearchEngine();
  const crawler = new CrawlEngine();

  // ===== Tab lifecycle events -> Daemon =====
  tabs.onTabCreated((tab) => {
    ws.send({
      type: 'TabCreated',
      tab_id: tab.id,
      url: tab.url,
      window_id: tab.windowId,
    });
  });

  tabs.onTabUpdated((tab) => {
    ws.setActiveTabId(tab.id);
    ws.send({
      type: 'TabUpdated',
      tab_id: tab.id,
      url: tab.url,
      title: tab.title,
      status: tab.status,
    });
  });

  tabs.onTabClosed((tab) => {
    ws.send({ type: 'TabClosed', tab_id: tab.id });
  });

  // ===== Crawl events -> Daemon =====
  crawler.setProgressCallback((jobId, pagesCrawled, pagesTotal, currentUrl) => {
    ws.send({ type: 'CrawlProgress', job_id: jobId, pages_crawled: pagesCrawled, pages_total: pagesTotal, current_url: currentUrl });
  });

  crawler.setResultCallback((jobId, pageResult) => {
    ws.send({ type: 'CrawlResult', job_id: jobId, page_result: pageResult });
  });

  crawler.setCompleteCallback((jobId, totalPages, durationMs) => {
    ws.send({ type: 'CrawlComplete', job_id: jobId, total_pages: totalPages, duration_ms: durationMs });
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
        ws.send({
          type: 'Response',
          request_id: (msg as any).request_id,
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

async function handleDaemonMessage(
  msg: DaemonMessage,
  tabs: TabsController,
  searcher: SearchEngine,
  crawler: CrawlEngine,
  ws: WSClient,
): Promise<void> {
  switch (msg.type) {
    // ===== Tab Management =====
    case 'Navigate': {
      logActivity(`Navigating to: ${msg.url}`);
      const tab = await tabs.navigate(msg.url, msg.tab_id);
      ws.send({ type: 'Response', request_id: msg.request_id, status: 'ok', data: { tab_id: tab.id, url: tab.url } });
      break;
    }
    case 'NewTab': {
      const tab = await tabs.create(msg.url);
      ws.send({ type: 'Response', request_id: msg.request_id, status: 'ok', data: { tab_id: tab.id } });
      break;
    }
    case 'CloseTab': {
      await tabs.close(msg.tab_id);
      ws.send({ type: 'Response', request_id: msg.request_id, status: 'ok', data: {} });
      break;
    }
    case 'SwitchTab': {
      await tabs.switchTo(msg.tab_id);
      ws.send({ type: 'Response', request_id: msg.request_id, status: 'ok', data: {} });
      break;
    }
    case 'GoBack': {
      await tabs.goBack(msg.tab_id);
      ws.send({ type: 'Response', request_id: msg.request_id, status: 'ok', data: {} });
      break;
    }
    case 'GoForward': {
      await tabs.goForward(msg.tab_id);
      ws.send({ type: 'Response', request_id: msg.request_id, status: 'ok', data: {} });
      break;
    }
    case 'Reload': {
      await tabs.reload(msg.tab_id);
      ws.send({ type: 'Response', request_id: msg.request_id, status: 'ok', data: {} });
      break;
    }
    case 'ListTabs': {
      ws.send({
        type: 'Response',
        request_id: msg.request_id,
        status: 'ok',
        data: tabs.listTabs(),
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
      logActivity(`Action: ${msg.type}`);
      const result = await MessageBridge.sendToTab(msg.tab_id ?? tabs.getActiveTabId() ?? undefined, msg);
      ws.send({ type: 'Response', request_id: msg.request_id, ...result });
      break;
    }

    // ===== Execute JS =====
    case 'ExecuteJs': {
      const result = await MessageBridge.sendToTab(msg.tab_id ?? tabs.getActiveTabId() ?? undefined, {
        type: 'ExecuteJs',
        script: msg.script,
      });
      ws.send({ type: 'Response', request_id: msg.request_id, ...result });
      break;
    }

    // ===== Wait =====
    case 'WaitFor': {
      const condition = msg.condition;
      if (condition.type === 'time') {
        await new Promise(r => setTimeout(r, condition.ms));
        ws.send({ type: 'Response', request_id: msg.request_id, status: 'ok', data: {} });
      } else if (condition.type === 'navigation') {
        const tabId = msg.tab_id ?? tabs.getActiveTabId();
        if (tabId) {
          await new Promise<void>((resolve) => {
            const timeout = setTimeout(resolve, condition.timeout_ms);
            const listener = (_tabId: number, changeInfo: chrome.tabs.TabChangeInfo) => {
              if (_tabId === parseInt(tabId) && changeInfo.status === 'complete') {
                clearTimeout(timeout);
                chrome.tabs.onUpdated.removeListener(listener);
                resolve();
              }
            };
            chrome.tabs.onUpdated.addListener(listener);
          });
        }
        ws.send({ type: 'Response', request_id: msg.request_id, status: 'ok', data: {} });
      } else {
        ws.send({ type: 'Response', request_id: msg.request_id, status: 'ok', data: { message: 'Wait condition handled by content script' } });
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

      const result = await MessageBridge.sendToTab(
        msg.tab_id ?? tabs.getActiveTabId() ?? undefined,
        payload as any,
      );
      ws.send({ type: 'Response', request_id: msg.request_id, ...result });
      break;
    }

    // ===== Search =====
    case 'Search': {
      // Create a fresh tab for search
      logActivity(`Searching: "${msg.query}" on ${msg.engine}`);
      const tab = await tabs.create();
      await new Promise(r => setTimeout(r, 500));
      const results = await searcher.search(tab.id, msg.query, msg.engine, msg.num_results, msg.language);
      ws.send({ type: 'Response', request_id: msg.request_id, status: 'ok', data: results });
      logActivity(`Search complete: ${results.results.length} results found`);
      await tabs.close(tab.id);
      break;
    }

    // ===== Crawl =====
    case 'CrawlStart': {
      logActivity(`Starting crawl: ${msg.start_url}`);
      crawler.start({
        job_id: msg.job_id,
        start_url: msg.start_url,
        depth: msg.depth,
        max_pages: msg.max_pages,
        link_patterns: msg.link_patterns,
        exclude_patterns: msg.exclude_patterns,
        same_domain: msg.same_domain,
        per_page_timeout_ms: 10000,
        wait_between_pages_ms: 1000,
      });
      ws.send({ type: 'Response', request_id: msg.job_id, status: 'ok', data: { job_id: msg.job_id, status: 'started' } });
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
      const result = await MessageBridge.sendToTab(msg.tab_id ?? tabs.getActiveTabId() ?? undefined, msg);
      ws.send({ type: 'Response', request_id: msg.request_id, ...result });
      break;
    }

    // ===== Status =====
    case 'GetStatus': {
      const [activeTab] = await chrome.tabs.query({ active: true, currentWindow: true });
      const allTabs = await chrome.tabs.query({});
      ws.send({
        type: 'Response',
        request_id: msg.request_id,
        status: 'ok',
        data: {
          connected: true,
          tab_count: allTabs.length,
          active_tab_id: activeTab?.id?.toString() ?? null,
        },
      });
      break;
    }

    default:
      console.warn('[SenClaw] Unknown message type:', (msg as any).type);
  }
}
