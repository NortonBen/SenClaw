// Tab group management - one tab per agent
// Each agent gets exactly one tab in the SenClaw group, reused for all operations
import type { TabId, AgentId } from '../types/protocol';

const GROUP_TITLE = 'SenClaw';
const GROUP_COLOR: chrome.tabGroups.Color = 'blue';

interface AgentTabInfo {
  tabId: number;
  agentId: AgentId;
  url: string;
  createdAt: number;
}

export class TabGroupController {
  private groupId: number | null = null;
  private agentTabs: Map<AgentId, AgentTabInfo> = new Map(); // agent_id -> tab info
  private tabToAgent: Map<number, AgentId> = new Map(); // tab_id -> agent_id

  /**
   * Get tab ID for an agent (if exists)
   */
  getAgentTabId(agentId: AgentId): TabId | null {
    const info = this.agentTabs.get(agentId);
    return info ? info.tabId.toString() : null;
  }

  /**
   * Check if a tab is managed by SenClaw (belongs to an agent)
   */
  isSenclawTab(tabId: TabId): boolean {
    return this.tabToAgent.has(parseInt(tabId));
  }

  /**
   * Get agent ID for a tab
   */
  getAgentForTab(tabId: TabId): AgentId | null {
    return this.tabToAgent.get(parseInt(tabId)) ?? null;
  }

  /**
   * Get all SenClaw tab IDs
   */
  getSenclawTabIds(): TabId[] {
    return Array.from(this.agentTabs.values()).map(info => info.tabId.toString());
  }

  /**
   * Get all agent-tab mappings
   */
  getAgentTabMappings(): { agentId: AgentId; tabId: TabId; url: string }[] {
    return Array.from(this.agentTabs.entries()).map(([agentId, info]) => ({
      agentId,
      tabId: info.tabId.toString(),
      url: info.url,
    }));
  }

  /**
   * Get current group ID
   */
  getGroupId(): number | null {
    return this.groupId;
  }

  /**
   * Create or get a tab for an agent
   * If agent already has a tab, return it. Otherwise create new tab in group.
   * @param active - Whether to focus the tab (default: true)
   */
  async getOrCreateTabForAgent(agentId: AgentId, url?: string, windowId?: number, active: boolean = false): Promise<chrome.tabs.Tab> {
    // Check if agent already has a tab
    const existing = this.agentTabs.get(agentId);
    if (existing) {
      try {
        // Verify tab still exists
        const tab = await chrome.tabs.get(existing.tabId);
        if (url && tab.url !== url) {
          // Navigate to new URL — never pass active:true when caller wants background
          await chrome.tabs.update(existing.tabId, { url, active });
        } else if (active) {
          // Only bring to front when explicitly requested
          await chrome.tabs.update(existing.tabId, { active: true });
        }
        // Update URL in tracking
        existing.url = url ?? tab.url ?? '';
        console.log(`[SenClaw] Reused tab ${existing.tabId} for agent ${agentId} (active: ${active})`);
        return tab;
      } catch {
        // Tab was closed, remove from tracking
        this.tabToAgent.delete(existing.tabId);
        this.agentTabs.delete(agentId);
      }
    }

    // Snapshot the currently active tab so we can restore focus if active=false
    let previousActiveTabId: number | null = null;
    if (!active) {
      const [cur] = await chrome.tabs.query({ active: true, lastFocusedWindow: true });
      if (cur?.id) previousActiveTabId = cur.id;
    }

    // Create new tab for this agent
    console.log(`[SenClaw] Creating new tab for agent ${agentId} (active: ${active})`);
    const tab = await chrome.tabs.create({
      url,
      windowId,
      active,
    });

    if (!tab.id) {
      throw new Error('Failed to create tab - no tab ID');
    }

    // Ensure group exists or create new one
    let targetGroupId = this.groupId ?? -1;

    if (targetGroupId === -1) {
      // Try to find existing SenClaw group
      const groups = await chrome.tabGroups.query({ title: GROUP_TITLE });
      if (groups.length > 0) {
        targetGroupId = groups[0].id;
      }
    }

    // Add tab to group (creates new group if targetGroupId is -1)
    // NOTE: chrome.tabs.group() can steal focus; restore below if needed
    const groupId = await chrome.tabs.group({
      tabIds: [tab.id],
      groupId: targetGroupId === -1 ? undefined : targetGroupId,
    });

    this.groupId = groupId;

    // Configure group appearance (only when creating new)
    if (targetGroupId === -1) {
      await chrome.tabGroups.update(groupId, {
        title: GROUP_TITLE,
        color: GROUP_COLOR,
        collapsed: false,
      });
    }

    // Restore focus to original tab if we were asked to open in background
    if (!active && previousActiveTabId !== null) {
      try {
        await chrome.tabs.update(previousActiveTabId, { active: true });
      } catch {
        // Previous tab may have been closed — ignore
      }
    }

    // Track this tab for the agent
    const tabInfo: AgentTabInfo = {
      tabId: tab.id,
      agentId,
      url: url ?? tab.url ?? '',
      createdAt: Date.now(),
    };
    this.agentTabs.set(agentId, tabInfo);
    this.tabToAgent.set(tab.id, agentId);

    return tab;
  }

  /**
   * Navigate a specific agent's tab
   * If agent doesn't have a tab, create one
   * @param active - Whether to focus the tab (default: true)
   */
  async navigateForAgent(agentId: AgentId, url: string, windowId?: number, active: boolean = false): Promise<chrome.tabs.Tab> {
    return this.getOrCreateTabForAgent(agentId, url, windowId, active);
  }

  /**
   * Close a specific agent's tab (only when explicitly requested)
   */
  async closeAgentTab(agentId: AgentId): Promise<void> {
    const info = this.agentTabs.get(agentId);
    if (!info) {
      throw new Error(`No tab found for agent ${agentId}`);
    }

    await chrome.tabs.remove(info.tabId);
    this.tabToAgent.delete(info.tabId);
    this.agentTabs.delete(agentId);
  }

  /**
   * Close a specific tab by ID (must be SenClaw managed tab)
   */
  async closeTab(tabId: TabId): Promise<void> {
    const agentId = this.tabToAgent.get(parseInt(tabId));
    if (!agentId) {
      throw new Error('Tab is not managed by SenClaw');
    }

    await this.closeAgentTab(agentId);
  }

  /**
   * Remove agent from tracking but DON'T close the tab
   * Tab becomes "orphaned" in the group but stays open
   */
  async releaseAgentTab(agentId: AgentId): Promise<void> {
    const info = this.agentTabs.get(agentId);
    if (info) {
      this.tabToAgent.delete(info.tabId);
      this.agentTabs.delete(agentId);
    }
  }

  /**
   * Remove all agent tracking but keep all tabs open
   */
  async releaseAllTabs(): Promise<void> {
    this.agentTabs.clear();
    this.tabToAgent.clear();
  }

  /**
   * Get tab info for an agent
   */
  getAgentTabInfo(agentId: AgentId): AgentTabInfo | null {
    return this.agentTabs.get(agentId) ?? null;
  }

  /**
   * List all agent tabs
   */
  listAgentTabs(): AgentTabInfo[] {
    return Array.from(this.agentTabs.values());
  }

  /**
   * Update URL for a tab (called when tab navigates)
   */
  updateTabUrl(tabId: number, url: string): void {
    const agentId = this.tabToAgent.get(tabId);
    if (agentId) {
      const info = this.agentTabs.get(agentId);
      if (info) {
        info.url = url;
      }
    }
  }

  /**
   * Sync tracked tabs with actual browser state
   * Remove closed tabs from tracking
   */
  async syncTrackedTabs(): Promise<void> {
    const allTabs = await chrome.tabs.query({});
    const existingTabIds = new Set(allTabs.map(t => t.id).filter(Boolean) as number[]);

    // Remove tracking for closed tabs
    for (const [agentId, info] of this.agentTabs) {
      if (!existingTabIds.has(info.tabId)) {
        this.tabToAgent.delete(info.tabId);
        this.agentTabs.delete(agentId);
      }
    }

    // Update groupId if needed
    if (this.groupId !== null) {
      try {
        await chrome.tabGroups.get(this.groupId);
      } catch {
        this.groupId = null;
      }
    }
  }

  /**
   * Setup listeners for tab and group changes
   */
  setupListeners(): void {
    // Listen for tab removal (user closed tab)
    chrome.tabs.onRemoved.addListener((tabId) => {
      const agentId = this.tabToAgent.get(tabId);
      if (agentId) {
        this.agentTabs.delete(agentId);
        this.tabToAgent.delete(tabId);
      }
    });

    // Listen for tab updates (navigation)
    chrome.tabs.onUpdated.addListener((tabId, changeInfo, tab) => {
      if (changeInfo.url && this.tabToAgent.has(tabId)) {
        this.updateTabUrl(tabId, changeInfo.url);
      }
    });

    // Listen for group removal
    chrome.tabGroups.onRemoved.addListener((group) => {
      if (group.id === this.groupId) {
        this.groupId = null;
      }
    });

    // Periodic sync to keep tracking accurate
    setInterval(() => this.syncTrackedTabs(), 5000);
  }
}
