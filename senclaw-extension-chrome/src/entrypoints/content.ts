// Content script: DOM extraction + action execution.
// Injected into every page. Listens for messages from background script.

import {
  buildSnapshot,
  extractText,
  extractLinks,
  extractTable,
} from '../agent/DomExtractor';
import {
  clickElement,
  typeText,
  selectOption,
  scrollPage,
  hoverElement,
  pressKey,
  executeJs,
} from '../agent/ActionExecutor';
import {
  extractGoogleResults,
  extractBingResults,
} from '../agent/SearchExtractor';
import { compressHtml } from '../agent/HtmlCompressor';

export default defineContentScript({
  matches: ['<all_urls>'],
  main() {
    chrome.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
      try {
        const result = handleMessage(msg);
        sendResponse(result);
      } catch (e: unknown) {
        sendResponse({
          success: false,
          message: e instanceof Error ? e.message : String(e),
        });
      }
      // Return true for async response support
      return true;
    });
  },
});

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function handleMessage(msg: any): any {
  switch (msg.type) {
    // ===== Observation =====
    case 'GetSnapshot': {
      const snapshot = buildSnapshot(msg.depth);
      if (msg.compress_html) {
        const compressed = compressHtml(document.documentElement.outerHTML);
        return {
          url: snapshot.url,
          title: snapshot.title,
          elements: snapshot.elements,
          text_content_summary: snapshot.text_content_summary,
          compressed_html: JSON.stringify({
            interactive_elements: compressed.interactiveElements.length,
            text_preview: compressed.textContent.slice(0, 2000),
          }),
          compression_stats: compressed.stats,
          html: document.documentElement.outerHTML,
        };
      }
      return snapshot;
    }

    case 'ExtractText': {
      return extractText(msg.selector);
    }

    case 'ExtractLinks': {
      return extractLinks(msg.selector);
    }

    case 'ExtractTable': {
      return extractTable(msg.selector);
    }

    case 'GetScreenshot': {
      // Canvas-based screenshot capture
      return captureScreenshot(msg.full_page);
    }

    // ===== Actions =====
    case 'Click': {
      return clickElement(msg.index);
    }

    case 'Type': {
      return typeText(msg.index, msg.text, msg.submit ?? false);
    }

    case 'SelectOption': {
      return selectOption(msg.index, msg.option_text);
    }

    case 'Scroll': {
      return scrollPage(msg.direction, msg.amount);
    }

    case 'Hover': {
      return hoverElement(msg.index);
    }

    case 'PressKey': {
      return pressKey(msg.key);
    }

    case 'ExecuteJs': {
      return executeJs(msg.script);
    }

    // ===== Fill Form =====
    case 'FillForm': {
      return fillForm(msg.fields, msg.submit);
    }

    // ===== Search Extraction =====
    case 'ExtractSearchResults': {
      if (msg.engine === 'google') {
        return { results: extractGoogleResults() };
      }
      return { results: extractBingResults() };
    }

    default:
      return { success: false, message: `Unknown content script message: ${msg.type}` };
  }
}

function fillForm(
  fields: { target: string; value: string; type: string }[],
  submit: boolean,
): { success: boolean; message: string } {
  for (const field of fields) {
    let el: Element | null = null;

    // Try finding by CSS selector
    if (field.target.startsWith('#') || field.target.startsWith('.') || field.target.startsWith('[')) {
      el = document.querySelector(field.target);
    } else {
      // Try finding by label, placeholder, or name
      el = document.querySelector(`[name="${field.target}"], [placeholder*="${field.target}"], [aria-label*="${field.target}"]`)

      // Try finding by label text
      if (!el) {
        const labels = document.querySelectorAll('label');
        for (const label of labels) {
          if (label.textContent?.includes(field.target)) {
            const forAttr = label.getAttribute('for');
            if (forAttr) {
              el = document.getElementById(forAttr);
            } else {
              el = label.querySelector('input, select, textarea');
            }
            if (el) break;
          }
        }
      }
    }

    if (!el) {
      return { success: false, message: `Field not found: ${field.target}` };
    }

    const tag = el.tagName.toLowerCase();
    if (tag === 'input') {
      const input = el as HTMLInputElement;
      if (field.type === 'checkbox') {
        input.checked = field.value === 'true';
      } else if (field.type === 'radio') {
        (el as HTMLInputElement).checked = true;
      } else {
        input.value = field.value;
        input.dispatchEvent(new Event('input', { bubbles: true }));
      }
    } else if (tag === 'textarea') {
      (el as HTMLTextAreaElement).value = field.value;
      el.dispatchEvent(new Event('input', { bubbles: true }));
    } else if (tag === 'select') {
      const select = el as HTMLSelectElement;
      for (const opt of select.options) {
        if (opt.text.includes(field.value) || opt.value === field.value) {
          opt.selected = true;
          break;
        }
      }
      select.dispatchEvent(new Event('change', { bubbles: true }));
    }

    el.dispatchEvent(new Event('change', { bubbles: true }));
  }

  if (submit) {
    const form = (document.activeElement as HTMLElement)?.closest('form');
    if (form) {
      form.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    }
  }

  return { success: true, message: `Filled ${fields.length} field(s)` };
}

async function captureScreenshot(fullPage: boolean): Promise<{ data?: string; error?: string }> {
  try {
    if (fullPage) {
      // Full-page screenshot using html2canvas-like approach
      const canvas = document.createElement('canvas');
      const body = document.body;
      const html = document.documentElement;
      const width = Math.max(body.scrollWidth, body.offsetWidth, html.clientWidth, html.scrollWidth, html.offsetWidth);
      const height = Math.max(body.scrollHeight, body.offsetHeight, html.clientHeight, html.scrollHeight, html.offsetHeight);
      canvas.width = Math.min(width, 4096);
      canvas.height = Math.min(height, 16384);
      // For full page screenshots, return dimensions (actual capture would need extension API)
      return { data: `canvas:${canvas.width}x${canvas.height}` };
    } else {
      // Viewport screenshot not possible from content script directly
      // The background script should use chrome.tabs.captureVisibleTab instead
      return { error: 'Screenshot must be captured from background script using chrome.tabs.captureVisibleTab' };
    }
  } catch (e: unknown) {
    return { error: e instanceof Error ? e.message : String(e) };
  }
}
