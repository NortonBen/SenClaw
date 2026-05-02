// Multi-tab lifecycle management.
import type { TabId } from '../types/protocol';

interface TabInfo {
  id: TabId;
  url: string;
  title: string;
  status: 'loading' | 'complete';
  windowId: number;
}

type TabEventCallback = (tab: TabInfo) => void;

export class TabsController {
  private tabs: Map<TabId, TabInfo> = new Map();
  private activeTabId: TabId | null = null;
  private onCreatedCb: TabEventCallback | null = null;
  private onUpdatedCb: TabEventCallback | null = null;
  private onClosedCb: TabEventCallback | null = null;

  constructor() {
    this.setupListeners();
  }

  async navigate(url: string, tabId?: TabId): Promise<TabInfo> {
    if (tabId) {
      const tab = await chrome.tabs.update(parseInt(tabId), { url, active: true });
      const info = this.fromChromeTab(tab);
      this.tabs.set(info.id, info);
      this.activeTabId = info.id;
      return info;
    }

    const tab = await chrome.tabs.create({ url, active: true });
    const info = this.fromChromeTab(tab);
    this.tabs.set(info.id, info);
    this.activeTabId = info.id;
    return info;
  }

  async create(url?: string): Promise<TabInfo> {
    const tab = await chrome.tabs.create({ url, active: true });
    const info = this.fromChromeTab(tab);
    this.tabs.set(info.id, info);
    this.activeTabId = info.id;
    return info;
  }

  async close(tabId: TabId): Promise<void> {
    await chrome.tabs.remove(parseInt(tabId));
    this.tabs.delete(tabId);
    if (this.activeTabId === tabId) {
      this.activeTabId = null;
    }
  }

  async switchTo(tabId: TabId): Promise<void> {
    await chrome.tabs.update(parseInt(tabId), { active: true });
    this.activeTabId = tabId;
  }

  async goBack(tabId?: TabId): Promise<void> {
    const id = tabId ?? this.activeTabId;
    if (id) await chrome.tabs.goBack(parseInt(id));
  }

  async goForward(tabId?: TabId): Promise<void> {
    const id = tabId ?? this.activeTabId;
    if (id) await chrome.tabs.goForward(parseInt(id));
  }

  async reload(tabId?: TabId): Promise<void> {
    const id = tabId ?? this.activeTabId;
    if (id) await chrome.tabs.reload(parseInt(id));
  }

  getActiveTabId(): TabId | null {
    return this.activeTabId;
  }

  getTab(tabId: TabId): TabInfo | undefined {
    return this.tabs.get(tabId);
  }

  listTabs(): TabInfo[] {
    return Array.from(this.tabs.values());
  }

  onTabCreated(cb: TabEventCallback): void { this.onCreatedCb = cb; }
  onTabUpdated(cb: TabEventCallback): void { this.onUpdatedCb = cb; }
  onTabClosed(cb: TabEventCallback): void { this.onClosedCb = cb; }

  private setupListeners(): void {
    chrome.tabs.onCreated.addListener((tab) => {
      if (!tab.id) return;
      const info = this.fromChromeTab(tab);
      this.tabs.set(info.id, info);
      this.onCreatedCb?.(info);
    });

    chrome.tabs.onUpdated.addListener((tabId, changeInfo, tab) => {
      if (changeInfo.status) {
        const info = this.fromChromeTab(tab);
        this.tabs.set(info.id, info);
        if (changeInfo.status === 'complete') {
          this.onUpdatedCb?.(info);
        }
      }
    });

    chrome.tabs.onRemoved.addListener((tabId) => {
      const idStr = tabId.toString();
      const info = this.tabs.get(idStr);
      this.tabs.delete(idStr);
      if (info) this.onClosedCb?.(info);
    });

    // Track active tab changes
    chrome.tabs.onActivated.addListener((activeInfo) => {
      this.activeTabId = activeInfo.tabId.toString();
    });

    // Initialize from current tabs
    chrome.tabs.query({}, (tabs) => {
      tabs.forEach((tab) => {
        if (tab.id) {
          this.tabs.set(tab.id.toString(), this.fromChromeTab(tab));
        }
      });
    });
    chrome.tabs.query({ active: true, currentWindow: true }, ([tab]) => {
      if (tab?.id) this.activeTabId = tab.id.toString();
    });
  }

  private fromChromeTab(tab: chrome.tabs.Tab): TabInfo {
    return {
      id: (tab.id ?? 0).toString(),
      url: tab.url ?? tab.pendingUrl ?? '',
      title: tab.title ?? '',
      status: tab.status === 'complete' ? 'complete' : 'loading',
      windowId: tab.windowId,
    };
  }
}
