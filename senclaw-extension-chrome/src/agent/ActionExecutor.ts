// Executes user actions (click, type, scroll) on page elements.

const INTERACTIVE_SELECTOR = [
  'a', 'button', 'input', 'select', 'textarea',
  '[role="button"]', '[role="link"]', '[role="textbox"]',
  '[role="combobox"]', '[role="checkbox"]', '[role="radio"]',
  '[tabindex]:not([tabindex="-1"])', '[contenteditable="true"]',
  'details', 'summary', '[onclick]',
].join(',');

function getElementByIndex(index: number): Element | null {
  const all = document.querySelectorAll(INTERACTIVE_SELECTOR);
  let visibleIdx = 0;
  for (const el of all) {
    const rect = el.getBoundingClientRect();
    if (rect.width === 0 || rect.height === 0) continue;
    if (visibleIdx === index) return el;
    visibleIdx++;
  }
  return null;
}

export interface ActionResult {
  success: boolean;
  message?: string;
  data?: unknown;
}

export function clickElement(index: number): ActionResult {
  const el = getElementByIndex(index);
  if (!el) return { success: false, message: `Element #${index} not found` };
  (el as HTMLElement).click();
  return { success: true, message: `Clicked element #${index}` };
}

export function typeText(index: number, text: string, submit: boolean): ActionResult {
  const el = getElementByIndex(index) as HTMLInputElement | HTMLTextAreaElement;
  if (!el) return { success: false, message: `Element #${index} not found` };

  el.focus();
  el.value = text;
  el.dispatchEvent(new Event('input', { bubbles: true }));
  el.dispatchEvent(new Event('change', { bubbles: true }));

  if (submit) {
    const form = el.closest('form');
    if (form) {
      form.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    } else {
      el.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
      el.dispatchEvent(new KeyboardEvent('keyup', { key: 'Enter', bubbles: true }));
    }
  }

  return { success: true, message: `Typed into element #${index}` };
}

export function selectOption(index: number, optionText: string): ActionResult {
  const el = getElementByIndex(index) as HTMLSelectElement;
  if (!el || el.tagName !== 'SELECT') {
    return { success: false, message: `Element #${index} is not a select` };
  }

  for (const opt of el.options) {
    if (opt.text.includes(optionText) || opt.value === optionText) {
      opt.selected = true;
      el.dispatchEvent(new Event('change', { bubbles: true }));
      return { success: true, message: `Selected "${optionText}"` };
    }
  }

  return { success: false, message: `Option "${optionText}" not found` };
}

export function scrollPage(direction: string, amount: { Pages?: number; Pixels?: number }): ActionResult {
  const px = amount.Pixels ?? ((amount.Pages ?? 1) * window.innerHeight);
  const delta = direction === 'up' ? -px : px;
  window.scrollBy({ top: delta, behavior: 'smooth' });
  return { success: true, message: `Scrolled ${direction}` };
}

export function hoverElement(index: number): ActionResult {
  const el = getElementByIndex(index) as HTMLElement;
  if (!el) return { success: false, message: `Element #${index} not found` };
  el.dispatchEvent(new MouseEvent('mouseenter', { bubbles: true }));
  el.dispatchEvent(new MouseEvent('mouseover', { bubbles: true }));
  return { success: true, message: `Hovered element #${index}` };
}

export function pressKey(key: string): ActionResult {
  const event = new KeyboardEvent('keydown', { key, bubbles: true });
  document.activeElement?.dispatchEvent(event);
  document.activeElement?.dispatchEvent(new KeyboardEvent('keyup', { key, bubbles: true }));
  return { success: true, message: `Pressed ${key}` };
}

export function executeJs(script: string): ActionResult {
  try {
    const fn = new Function(script);
    const result = fn();
    return { success: true, data: result };
  } catch (e: unknown) {
    return { success: false, message: e instanceof Error ? e.message : String(e) };
  }
}
