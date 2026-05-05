// Multi-tab lifecycle management with SenClaw group tracking (one tab per agent).
import type { TabId, AgentId } from '../types/protocol';
import { TabGroupController } from './TabGroupController';

interface TabInfo {
  id: TabId;
  url: string;
  title: string;
  status: 'loading' | 'complete';
  windowId: number;
  groupId?: number;
  agentId?: AgentId;
  isSenclawTab: boolean;
}

type TabEventCallback = (tab: TabInfo) => void;

export class TabsController {
  private tabs: Map<TabId, TabInfo> = new Map();
  private activeTabId: TabId | null = null;
  private onCreatedCb: TabEventCallback | null = null;
  private onUpdatedCb: TabEventCallback | null = null;
  private onClosedCb: TabEventCallback | null = null;
  readonly groupController: TabGroupController;

  constructor(groupController: TabGroupController) {
    this.groupController = groupController;
    this.setupListeners();
  }

  /**
   * Navigate to URL for a specific agent
   * Reuses the agent's existing tab or creates new one
   * @param active - Whether to focus the tab (default: false — background like Claude)
   */
  async navigateForAgent(agentId: AgentId, url: string, windowId?: number, active: boolean = false): Promise<TabInfo> {
    const tab = await this.groupController.navigateForAgent(agentId, url, windowId, active);
    const info = this.fromChromeTab(tab, agentId);
    this.tabs.set(info.id, info);
    if (active) {
      this.activeTabId = info.id;
    }
    return info;
  }

  /**
   * Create/get tab for a specific agent
   * @param active - Whether to focus the tab (default: false — background like Claude)
   */
  async getOrCreateForAgent(agentId: AgentId, url?: string, active: boolean = false): Promise<TabInfo> {
    const tab = await this.groupController.getOrCreateTabForAgent(agentId, url, undefined, active);
    const info = this.fromChromeTab(tab, agentId);
    this.tabs.set(info.id, info);
    if (active) {
      this.activeTabId = info.id;
    }
    return info;
  }

  /**
   * Navigate to URL (legacy method - uses group controller default)
   */
  async navigate(url: string, tabId?: TabId): Promise<TabInfo> {
    // If tabId is provided and is SenClaw tab, navigate it
    if (tabId && this.groupController.isSenclawTab(tabId)) {
      const agentId = this.groupController.getAgentForTab(tabId);
      if (agentId) {
        return this.navigateForAgent(agentId, url);
      }
      const tab = await chrome.tabs.update(parseInt(tabId), { url, active: true });
      const info = this.fromChromeTab(tab);
      this.tabs.set(info.id, info);
      this.activeTabId = info.id;
      return info;
    }

    // Fallback: create anonymous tab (shouldn't happen in normal flow)
    const tab = await chrome.tabs.create({ url, active: true });
    const info = this.fromChromeTab(tab);
    this.tabs.set(info.id, info);
    this.activeTabId = info.id;
    return info;
  }

  /**
   * Create new tab (legacy method)
   */
  async create(url?: string): Promise<TabInfo> {
    const tab = await chrome.tabs.create({ url, active: true });
    const info = this.fromChromeTab(tab);
    this.tabs.set(info.id, info);
    this.activeTabId = info.id;
    return info;
  }

  /**
   * Close a SenClaw tab for a specific agent
   */
  async closeForAgent(agentId: AgentId): Promise<void> {
    const tabId = this.groupController.getAgentTabId(agentId);
    if (!tabId) {
      throw new Error(`No tab found for agent ${agentId}`);
    }

    await this.groupController.closeAgentTab(agentId);
    this.tabs.delete(tabId);
    if (this.activeTabId === tabId) {
      this.activeTabId = null;
    }
  }

  /**
   * Close a SenClaw tab (legacy method)
   */
  async close(tabId: TabId): Promise<void> {
    if (!this.groupController.isSenclawTab(tabId)) {
      throw new Error('Tab is not managed by SenClaw');
    }

    await this.groupController.closeTab(tabId);
    this.tabs.delete(tabId);
    if (this.activeTabId === tabId) {
      this.activeTabId = null;
    }
  }

  /**
   * Release agent's tab (remove from tracking but keep tab open)
   */
  async releaseAgentTab(agentId: AgentId): Promise<void> {
    const tabId = this.groupController.getAgentTabId(agentId);
    await this.groupController.releaseAgentTab(agentId);
    if (tabId) {
      const info = this.tabs.get(tabId);
      if (info) {
        info.isSenclawTab = false;
        info.agentId = undefined;
        this.tabs.set(tabId, info);
      }
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

  /**
   * List ALL tabs (both SenClaw and non-SenClaw)
   */
  listTabs(): TabInfo[] {
    return Array.from(this.tabs.values());
  }

  /**
   * List only SenClaw managed tabs
   */
  listSenclawTabs(): TabInfo[] {
    return Array.from(this.tabs.values()).filter(t => t.isSenclawTab);
  }

  /**
   * List tabs for a specific agent
   */
  listTabsForAgent(agentId: AgentId): TabInfo[] {
    return Array.from(this.tabs.values()).filter(t => t.agentId === agentId);
  }

  /**
   * Check if tab is managed by SenClaw
   */
  isSenclawTab(tabId: TabId): boolean {
    return this.groupController.isSenclawTab(tabId);
  }

  /**
   * Get agent ID for a tab
   */
  getAgentForTab(tabId: TabId): AgentId | null {
    return this.groupController.getAgentForTab(tabId);
  }

  onTabCreated(cb: TabEventCallback): void { this.onCreatedCb = cb; }
  onTabUpdated(cb: TabEventCallback): void { this.onUpdatedCb = cb; }
  onTabClosed(cb: TabEventCallback): void { this.onClosedCb = cb; }

  private setupListeners(): void {
    chrome.tabs.onCreated.addListener((tab) => {
      if (!tab.id) return;
      const info = this.fromChromeTab(tab);
      this.tabs.set(info.id, info);

      // Only notify daemon for SenClaw tabs
      if (info.isSenclawTab) {
        this.onCreatedCb?.(info);
      }
    });

    chrome.tabs.onUpdated.addListener((tabId, changeInfo, tab) => {
      const idStr = tabId.toString();
      const existing = this.tabs.get(idStr);

      if (changeInfo.status || changeInfo.url || changeInfo.title) {
        const info = this.fromChromeTab(tab);
        this.tabs.set(info.id, info);

        // Only notify daemon for SenClaw tabs when complete
        if (info.isSenclawTab && changeInfo.status === 'complete') {
          this.onUpdatedCb?.(info);
        }
      }
    });

    chrome.tabs.onRemoved.addListener((tabId) => {
      const idStr = tabId.toString();
      const info = this.tabs.get(idStr);

      if (info) {
        this.tabs.delete(idStr);
        // Only notify daemon for SenClaw tabs
        if (info.isSenclawTab) {
          this.onClosedCb?.(info);
        }
      }
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

  private fromChromeTab(tab: chrome.tabs.Tab, agentId?: AgentId): TabInfo {
    const id = (tab.id ?? 0).toString();
    const isSenclaw = agentId ? true : (this.groupController?.isSenclawTab(id) ?? false);
    const detectedAgentId = agentId ?? this.groupController?.getAgentForTab(id) ?? undefined;

    return {
      id,
      url: tab.url ?? tab.pendingUrl ?? '',
      title: tab.title ?? '',
      status: tab.status === 'complete' ? 'complete' : 'loading',
      windowId: tab.windowId,
      groupId: tab.groupId,
      agentId: detectedAgentId,
      isSenclawTab: isSenclaw,
    };
  }
}
