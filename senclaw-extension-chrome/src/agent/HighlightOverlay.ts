// Visual highlight overlay — numbered badges over interactive elements.
//
// Useful for debug, demo screenshots, or visual verification that the LLM
// is targeting the right element. Off by default; toggled via the snapshot param.
//
// Design:
//   - one fixed-position root container with id senclaw-highlights, tagged
//     data-senclaw-ignore so the tree walker skips it.
//   - one absolute box + label per interactive element.
//   - rAF-throttled reposition listener on scroll/resize.
//   - clear() removes everything in one go.

const ROOT_ID = 'senclaw-highlights';
const COLORS = [
  '#FF5252', '#FFAB40', '#FFEB3B', '#69F0AE',
  '#40C4FF', '#7C4DFF', '#FF4081', '#536DFE',
];

interface Tracked {
  index: number;
  el: HTMLElement;
  box: HTMLDivElement;
  label: HTMLDivElement;
}

let tracked: Tracked[] = [];
let root: HTMLDivElement | null = null;
let rafScheduled = false;

function ensureRoot(): HTMLDivElement {
  const existing = document.getElementById(ROOT_ID) as HTMLDivElement | null;
  if (existing) return existing;
  const el = document.createElement('div');
  el.id = ROOT_ID;
  el.setAttribute('data-senclaw-ignore', 'true');
  Object.assign(el.style, {
    position: 'fixed',
    inset: '0',
    pointerEvents: 'none',
    zIndex: '2147483647',
  } as Partial<CSSStyleDeclaration>);
  document.documentElement.appendChild(el);
  return el;
}

function reposition() {
  rafScheduled = false;
  for (const t of tracked) {
    if (!t.el.isConnected) continue;
    const r = t.el.getBoundingClientRect();
    Object.assign(t.box.style, {
      left: `${r.left}px`,
      top: `${r.top}px`,
      width: `${r.width}px`,
      height: `${r.height}px`,
    });
    Object.assign(t.label.style, {
      left: `${r.left}px`,
      top: `${Math.max(0, r.top - 16)}px`,
    });
  }
}

function scheduleReposition() {
  if (rafScheduled) return;
  rafScheduled = true;
  requestAnimationFrame(reposition);
}

let listenersBound = false;
function bindListeners() {
  if (listenersBound) return;
  window.addEventListener('scroll', scheduleReposition, { passive: true, capture: true });
  window.addEventListener('resize', scheduleReposition, { passive: true });
  listenersBound = true;
}

export function clearHighlights(): void {
  if (root) root.innerHTML = '';
  tracked = [];
}

export interface HighlightTarget {
  index: number;
  el: HTMLElement;
}

export function drawHighlights(targets: HighlightTarget[]): void {
  clearHighlights();
  if (targets.length === 0) return;
  root = ensureRoot();
  bindListeners();
  for (const t of targets) {
    const color = COLORS[t.index % COLORS.length];
    const r = t.el.getBoundingClientRect();
    const box = document.createElement('div');
    Object.assign(box.style, {
      position: 'fixed',
      left: `${r.left}px`,
      top: `${r.top}px`,
      width: `${r.width}px`,
      height: `${r.height}px`,
      border: `2px solid ${color}`,
      background: `${color}22`,
      borderRadius: '2px',
      pointerEvents: 'none',
      boxSizing: 'border-box',
    } as Partial<CSSStyleDeclaration>);
    const label = document.createElement('div');
    label.textContent = String(t.index);
    Object.assign(label.style, {
      position: 'fixed',
      left: `${r.left}px`,
      top: `${Math.max(0, r.top - 16)}px`,
      background: color,
      color: '#000',
      fontFamily: 'monospace',
      fontSize: '12px',
      fontWeight: 'bold',
      padding: '0 4px',
      borderRadius: '2px',
      lineHeight: '16px',
      pointerEvents: 'none',
    } as Partial<CSSStyleDeclaration>);
    root.appendChild(box);
    root.appendChild(label);
    tracked.push({ index: t.index, el: t.el, box, label });
  }
}
