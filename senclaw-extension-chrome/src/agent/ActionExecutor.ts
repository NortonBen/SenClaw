// Page actions — addressing elements by stable `index` resolved through SelectorMap.
//
// Each action dispatches the full DOM event sequence a real user would generate
// (pointer + mouse + focus + click) so React/Vue/Lit synthetic-event handlers
// fire correctly. Inputs go through the React-setter bypass trick so controlled
// components actually update state.

import { getElementByIndex } from './SelectorMap';

export interface ActionResult {
  success: boolean;
  message?: string;
  data?: unknown;
}

// ===== Helpers =====

function elementCenter(el: Element): { x: number; y: number } {
  const r = el.getBoundingClientRect();
  return { x: r.left + r.width / 2, y: r.top + r.height / 2 };
}

function pointerEvtInit(el: Element, type: string): PointerEventInit {
  const { x, y } = elementCenter(el);
  const r = el.getBoundingClientRect();
  return {
    bubbles: type !== 'pointerenter' && type !== 'mouseenter',
    cancelable: true,
    composed: true,
    button: 0,
    buttons: type === 'pointerdown' || type === 'mousedown' ? 1 : 0,
    clientX: x,
    clientY: y,
    screenX: x,
    screenY: y,
    pointerId: 1,
    pointerType: 'mouse',
    isPrimary: true,
    width: r.width,
    height: r.height,
  };
}

function dispatchSequence(el: Element, types: string[]): void {
  for (const type of types) {
    const init = pointerEvtInit(el, type);
    let evt: Event;
    if (type.startsWith('pointer')) {
      evt = new PointerEvent(type, init);
    } else if (type === 'focus') {
      try {
        (el as HTMLElement).focus({ preventScroll: true });
      } catch {
        (el as HTMLElement).focus();
      }
      continue;
    } else {
      evt = new MouseEvent(type, init as MouseEventInit);
    }
    el.dispatchEvent(evt);
  }
}

/**
 * Resolve the deepest visible descendant at the element's center.
 * If the page-agent overlay (or any decorative wrapper) is sitting on top of
 * the real target, fall back to the requested element.
 */
function deepestAt(el: Element): Element {
  const { x, y } = elementCenter(el);
  const hit = document.elementFromPoint(x, y);
  if (!hit) return el;
  if (hit === el || el.contains(hit)) return hit;
  return el;
}

/** True if this element's value is exposed via the React-tracked HTMLInputElement.value setter. */
function isNativeInput(el: Element): el is HTMLInputElement | HTMLTextAreaElement {
  const tag = el.tagName.toLowerCase();
  return tag === 'input' || tag === 'textarea';
}

/** React/Lit "tracked value" bypass: set value via the native prototype setter
 *  so the synthetic-event system sees the new value, then dispatch input+change. */
function setNativeValue(el: HTMLInputElement | HTMLTextAreaElement, value: string) {
  const proto = Object.getPrototypeOf(el);
  const desc = Object.getOwnPropertyDescriptor(proto, 'value');
  if (desc && desc.set) {
    desc.set.call(el, value);
  } else {
    el.value = value;
  }
}

function scrollIntoViewIfNeeded(el: Element) {
  const r = el.getBoundingClientRect();
  if (r.top < 0 || r.bottom > window.innerHeight || r.width === 0) {
    try {
      el.scrollIntoView({ block: 'center', inline: 'center', behavior: 'auto' });
    } catch {
      /* ignore */
    }
  }
}

function notFound(index: number): ActionResult {
  return {
    success: false,
    message: `Element #${index} not found in selector map. The page may have changed since the last snapshot — take a new snapshot.`,
  };
}

// ===== Actions =====

export function clickElement(index: number): ActionResult {
  const el = getElementByIndex(index);
  if (!el) return notFound(index);
  scrollIntoViewIfNeeded(el);
  const target = deepestAt(el);
  try {
    dispatchSequence(target, [
      'pointerover',
      'pointerenter',
      'mouseover',
      'mouseenter',
      'pointermove',
      'mousemove',
      'pointerdown',
      'mousedown',
      'focus',
      'pointerup',
      'mouseup',
    ]);
    (target as HTMLElement).click();
    return { success: true, message: `Clicked element #${index}` };
  } catch (e: unknown) {
    return {
      success: false,
      message: e instanceof Error ? e.message : String(e),
    };
  }
}

export function typeText(index: number, text: string, submit: boolean): ActionResult {
  const el = getElementByIndex(index);
  if (!el) return notFound(index);
  scrollIntoViewIfNeeded(el);

  try {
    (el as HTMLElement).focus({ preventScroll: true });
  } catch {
    (el as HTMLElement).focus();
  }

  try {
    if (isNativeInput(el)) {
      setNativeValue(el, text);
      el.dispatchEvent(new InputEvent('input', { bubbles: true, data: text, inputType: 'insertText' }));
      el.dispatchEvent(new Event('change', { bubbles: true }));
    } else if (el.getAttribute('contenteditable') && el.getAttribute('contenteditable') !== 'false') {
      // Contenteditable: select all, delete, insert
      const sel = window.getSelection();
      const range = document.createRange();
      range.selectNodeContents(el);
      sel?.removeAllRanges();
      sel?.addRange(range);
      el.dispatchEvent(new InputEvent('beforeinput', { bubbles: true, inputType: 'deleteContentBackward' }));
      (el as HTMLElement).innerText = '';
      el.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'deleteContentBackward' }));
      // Insert
      el.dispatchEvent(new InputEvent('beforeinput', { bubbles: true, data: text, inputType: 'insertText' }));
      (el as HTMLElement).innerText = text;
      el.dispatchEvent(new InputEvent('input', { bubbles: true, data: text, inputType: 'insertText' }));
      // Verify; if rich-text frameworks ignored us, fall back to execCommand
      if (((el as HTMLElement).innerText ?? '').trim() !== text.trim()) {
        try {
          document.execCommand('selectAll', false);
          document.execCommand('delete', false);
          document.execCommand('insertText', false, text);
        } catch {
          /* ignore */
        }
      }
    } else {
      return {
        success: false,
        message: `Element #${index} is not an input or contenteditable`,
      };
    }

    if (submit) {
      // Enter key dispatch (some forms listen for it)
      const enterInit: KeyboardEventInit = { key: 'Enter', code: 'Enter', bubbles: true, cancelable: true };
      el.dispatchEvent(new KeyboardEvent('keydown', enterInit));
      el.dispatchEvent(new KeyboardEvent('keypress', enterInit));
      el.dispatchEvent(new KeyboardEvent('keyup', enterInit));
      const form = (el as HTMLElement).closest('form') as HTMLFormElement | null;
      if (form) {
        if (typeof form.requestSubmit === 'function') {
          form.requestSubmit();
        } else {
          form.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
        }
      }
    }

    return { success: true, message: `Typed into element #${index}` };
  } catch (e: unknown) {
    return {
      success: false,
      message: e instanceof Error ? e.message : String(e),
    };
  }
}

export function selectOption(index: number, optionText: string): ActionResult {
  const el = getElementByIndex(index);
  if (!el) return notFound(index);

  if (el.tagName.toLowerCase() === 'select') {
    const select = el as HTMLSelectElement;
    for (const opt of Array.from(select.options)) {
      if (opt.text.includes(optionText) || opt.value === optionText) {
        select.value = opt.value;
        select.dispatchEvent(new Event('input', { bubbles: true }));
        select.dispatchEvent(new Event('change', { bubbles: true }));
        return { success: true, message: `Selected "${optionText}"` };
      }
    }
    return { success: false, message: `Option "${optionText}" not found in <select>` };
  }

  // ARIA combobox / listbox pattern: click to open, find option by text, click it
  if (
    el.getAttribute('role') === 'combobox' ||
    el.getAttribute('aria-haspopup') === 'listbox'
  ) {
    (el as HTMLElement).click();
    // Search visible listbox options
    const opts = document.querySelectorAll('[role="option"]');
    for (const opt of Array.from(opts)) {
      if ((opt.textContent ?? '').includes(optionText)) {
        (opt as HTMLElement).click();
        return { success: true, message: `Selected "${optionText}" via combobox` };
      }
    }
    return { success: false, message: `Option "${optionText}" not found in combobox` };
  }

  return { success: false, message: `Element #${index} is not a select or combobox` };
}

export function scrollPage(
  direction: string,
  amount: { Pages?: number; Pixels?: number; pages?: number; pixels?: number },
  containerIndex?: number,
): ActionResult {
  const px = amount.Pixels ?? amount.pixels ?? ((amount.Pages ?? amount.pages ?? 1) * window.innerHeight);
  const delta = direction === 'up' ? -px : px;

  if (containerIndex != null) {
    const el = getElementByIndex(containerIndex);
    if (el) {
      el.scrollBy({ top: delta, behavior: 'smooth' });
      return { success: true, message: `Scrolled container #${containerIndex} ${direction}` };
    }
  }

  window.scrollBy({ top: delta, behavior: 'smooth' });
  return { success: true, message: `Scrolled ${direction} by ${px}px` };
}

export function hoverElement(index: number): ActionResult {
  const el = getElementByIndex(index);
  if (!el) return notFound(index);
  scrollIntoViewIfNeeded(el);
  dispatchSequence(el, ['pointerover', 'pointerenter', 'mouseover', 'mouseenter', 'pointermove', 'mousemove']);
  return { success: true, message: `Hovered element #${index}` };
}

export function pressKey(key: string): ActionResult {
  const target = (document.activeElement as HTMLElement) ?? document.body;
  const init: KeyboardEventInit = { key, code: key, bubbles: true, cancelable: true };
  target.dispatchEvent(new KeyboardEvent('keydown', init));
  target.dispatchEvent(new KeyboardEvent('keypress', init));
  target.dispatchEvent(new KeyboardEvent('keyup', init));
  return { success: true, message: `Pressed ${key}` };
}

export function executeJs(script: string): ActionResult {
  try {
    // Wrap so the script can use `await` at the top level.
    const fn = new Function(
      `return (async () => { ${script.includes('return') ? script : `return ${script}`} })()`,
    );
    const result = fn();
    if (result instanceof Promise) {
      // Synchronous return path can't await — so the caller (content.ts handler)
      // should know this is fire-and-forget. We attach to a global slot for retrieval.
      return { success: true, data: '__pending_promise__' };
    }
    return { success: true, data: result };
  } catch (e: unknown) {
    return {
      success: false,
      message: e instanceof Error ? e.message : String(e),
    };
  }
}

export async function executeJsAsync(script: string): Promise<ActionResult> {
  try {
    const fn = new Function(`return (async () => { ${script} })()`);
    const result = await fn();
    return { success: true, data: result };
  } catch (e: unknown) {
    return {
      success: false,
      message: e instanceof Error ? e.message : String(e),
    };
  }
}
