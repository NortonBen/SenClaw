// Message bridge: relays messages between daemon and content scripts in tabs.
import type { DaemonMessage, TabId, RequestId, ActionResult } from '../types/protocol';

export class MessageBridge {
  /**
   * Send a message to a content script in a specific tab and return the response.
   * Falls back to the active tab if no tab_id is given.
   */
  static async sendToTab(
    tabId: TabId | undefined,
    msg: Omit<DaemonMessage, 'tab_id' | 'request_id'> & { request_id?: RequestId },
  ): Promise<ActionResult> {
    const targetTabId = tabId ?? await MessageBridge.getActiveTabId();

    if (!targetTabId) {
      return { status: 'error', message: 'No active tab' };
    }

    try {
      const response = await chrome.tabs.sendMessage(
        parseInt(targetTabId),
        msg,
      );
      return { status: 'ok', data: response };
    } catch (e: unknown) {
      const message = e instanceof Error ? e.message : String(e);
      // If the content script isn't ready, inject it
      if (message.includes('Could not establish connection') || message.includes('receiving end does not exist')) {
        return { status: 'error', message: 'Content script not loaded in tab, try reloading the page' };
      }
      return { status: 'error', message };
    }
  }

  private static async getActiveTabId(): Promise<TabId | null> {
    const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
    return tab?.id?.toString() ?? null;
  }
}
