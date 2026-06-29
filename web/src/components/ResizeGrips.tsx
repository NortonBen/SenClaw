import React from 'react';

// Resize grips for the borderless chat popover (decorations: false).
//
// On macOS a window with `decorations: false` has no native resize border, so
// even though tauri.conf.json sets `resizable: true` the user can't drag its
// edges. We overlay thin invisible hit-areas around the viewport and hand the
// drag off to Tauri's `startResizeDragging`, which performs a native resize.
//
// Outside the desktop shell (`window.__TAURI__` undefined — i.e. a plain
// browser tab) this renders nothing, so it's a no-op for the web UI.

type ResizeDirection =
  | 'North' | 'South' | 'East' | 'West'
  | 'NorthEast' | 'NorthWest' | 'SouthEast' | 'SouthWest';

interface TauriWindowGlobal {
  window?: {
    getCurrentWindow?: () => { startResizeDragging?: (dir: ResizeDirection) => Promise<void> };
  };
}

function tauriWindow(): TauriWindowGlobal['window'] | undefined {
  return (window as unknown as { __TAURI__?: TauriWindowGlobal }).__TAURI__?.window;
}

const EDGE = 6;    // edge hit-area thickness (px)
const CORNER = 14; // corner hit-area size (px)

interface Grip {
  dir: ResizeDirection;
  cursor: string;
  style: React.CSSProperties;
}

// Edges first, then corners (corners sit above edges via z-index so the
// diagonal cursor wins in the overlap zone).
const GRIPS: Grip[] = [
  { dir: 'North', cursor: 'ns-resize', style: { top: 0, left: CORNER, right: CORNER, height: EDGE } },
  { dir: 'South', cursor: 'ns-resize', style: { bottom: 0, left: CORNER, right: CORNER, height: EDGE } },
  { dir: 'West',  cursor: 'ew-resize', style: { left: 0, top: CORNER, bottom: CORNER, width: EDGE } },
  { dir: 'East',  cursor: 'ew-resize', style: { right: 0, top: CORNER, bottom: CORNER, width: EDGE } },
  { dir: 'NorthWest', cursor: 'nwse-resize', style: { top: 0, left: 0, width: CORNER, height: CORNER } },
  { dir: 'NorthEast', cursor: 'nesw-resize', style: { top: 0, right: 0, width: CORNER, height: CORNER } },
  { dir: 'SouthWest', cursor: 'nesw-resize', style: { bottom: 0, left: 0, width: CORNER, height: CORNER } },
  { dir: 'SouthEast', cursor: 'nwse-resize', style: { bottom: 0, right: 0, width: CORNER, height: CORNER } },
];

export function ResizeGrips() {
  const win = tauriWindow();
  if (!win?.getCurrentWindow) return null;

  const onGripDown = (dir: ResizeDirection) => (e: React.PointerEvent) => {
    if (e.button !== 0) return; // left button only
    e.preventDefault();
    try {
      win.getCurrentWindow!()?.startResizeDragging?.(dir);
    } catch {
      // Best-effort: if the window plugin command is unavailable we just no-op.
    }
  };

  return (
    <>
      {GRIPS.map(g => (
        <div
          key={g.dir}
          onPointerDown={onGripDown(g.dir)}
          style={{
            position: 'fixed',
            zIndex: 10000,
            cursor: g.cursor,
            ...g.style,
          }}
        />
      ))}
    </>
  );
}
