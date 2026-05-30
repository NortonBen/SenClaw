import { useEffect, useMemo, useState, useRef, useCallback } from 'react';
import type { WorkbenchState } from '../types';
import { StaticRenderer } from './workbench/StaticRenderer';
import { WebRenderer } from './workbench/WebRenderer';
import { BackendRenderer } from './workbench/BackendRenderer';
import { HistoryList } from './workbench/HistoryList';

// Drag-to-resize width, persisted to localStorage (mirrors AgentConsole).
const WIDTH_KEY = 'senclaw:workbench:width';
const DEFAULT_W = 480;
const MIN_W = 320;
const MAX_W = 960;

function loadWidth(): number {
  try {
    const v = localStorage.getItem(WIDTH_KEY);
    if (v) {
      const n = parseInt(v, 10);
      if (n >= MIN_W && n <= MAX_W) return n;
    }
  } catch {}
  return DEFAULT_W;
}

function saveWidth(w: number) {
  try { localStorage.setItem(WIDTH_KEY, String(w)); } catch {}
}

interface Props {
  /** Workbench state for the currently active jid. */
  state: WorkbenchState | null;
  /** Whether expanded (mutex controlled by the layout). */
  expanded: boolean;
  onCollapse: () => void;
  /** Read a file's content from the backend. */
  readFile: (artifactId: string, path: string) => Promise<{ content?: string; error?: string }>;
  /** Close an artifact. */
  closeArtifact: (artifactId: string) => void;
  /** Promote an artifact to current. */
  selectArtifact: (artifactId: string) => void;
  /** Mark as viewed (updates last_active). */
  markViewed: (artifactId: string) => void;
}

export function Workbench({ state, expanded, onCollapse, readFile, closeArtifact, selectArtifact, markViewed }: Props) {
  const current = state?.current ?? null;
  const history = state?.history ?? [];

  // Notify the backend "viewed" when current changes.
  useEffect(() => {
    if (expanded && current) markViewed(current.id);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [expanded, current?.id]);

  const readFileForCurrent = useMemo(
    () => (path: string) => current ? readFile(current.id, path) : Promise.resolve({ error: 'no_current' }),
    [current?.id, readFile],
  );

  // ===== Resize (drag the left edge), like AgentConsole =====
  const [width, setWidth] = useState(loadWidth);
  const widthRef = useRef(width);
  widthRef.current = width;
  const dragging = useRef(false);
  const dragStartX = useRef(0);
  const dragStartW = useRef(0);

  const onMouseDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    dragging.current = true;
    dragStartX.current = e.clientX;
    dragStartW.current = widthRef.current;
    document.body.style.cursor = 'col-resize';
    document.body.style.userSelect = 'none';
  }, []);

  useEffect(() => {
    const onMouseMove = (e: MouseEvent) => {
      if (!dragging.current) return;
      const delta = dragStartX.current - e.clientX; // panel is on the right: drag left → wider
      const next = Math.min(MAX_W, Math.max(MIN_W, dragStartW.current + delta));
      widthRef.current = next;
      setWidth(next);
    };
    const onMouseUp = () => {
      if (dragging.current) {
        dragging.current = false;
        document.body.style.cursor = '';
        document.body.style.userSelect = '';
        saveWidth(widthRef.current);
      }
    };
    window.addEventListener('mousemove', onMouseMove);
    window.addEventListener('mouseup', onMouseUp);
    return () => {
      window.removeEventListener('mousemove', onMouseMove);
      window.removeEventListener('mouseup', onMouseUp);
    };
  }, []);

  // Render nothing when collapsed (the badge is rendered by DockBadges).
  if (!expanded) return null;

  return (
    <div
      className="relative flex flex-col flex-shrink-0 border-l border-gray-100 bg-[#F5F8FB] overflow-hidden"
      style={{ width }}
    >
      {/* Header */}
      <div className="flex items-center justify-between px-3 py-2 border-b border-gray-200 bg-white flex-shrink-0">
        <div className="flex items-center gap-2 min-w-0">
          <span className="text-sm font-semibold text-gray-700 truncate">
            {current ? current.title : 'Workbench'}
          </span>
          {current && (
            <span className="text-[10px] text-gray-400 font-mono">{current.mode}</span>
          )}
        </div>
        <div className="flex items-center gap-1 flex-shrink-0">
          {current && (
            <button
              onClick={() => closeArtifact(current.id)}
              className="text-gray-400 hover:text-red-500 text-xs px-1.5 py-0.5 rounded hover:bg-gray-100"
              title="Close this workbench"
            >
              ✕ close
            </button>
          )}
          <button
            onClick={onCollapse}
            className="text-gray-400 hover:text-gray-600 text-xs px-1.5 py-0.5 rounded hover:bg-gray-100"
          >
            Hide ▸
          </button>
        </div>
      </div>

      {/* Body */}
      <div className="flex-1 min-h-0 overflow-hidden">
        {!current ? (
          <div className="flex h-full items-center justify-center text-xs text-gray-400 px-4 text-center">
            No content yet
          </div>
        ) : current.mode === 'static' ? (
          <StaticRenderer artifact={current} readFile={readFileForCurrent} />
        ) : current.mode === 'web' ? (
          <WebRenderer artifact={current} />
        ) : (
          <BackendRenderer artifact={current} />
        )}
      </div>

      {/* History */}
      <HistoryList
        items={history}
        currentId={current?.id ?? null}
        onSelect={selectArtifact}
        onClose={closeArtifact}
      />

      {/* Resize handle (left edge) */}
      <div
        onMouseDown={onMouseDown}
        className="absolute top-0 left-0 h-full"
        style={{ width: '4px', cursor: 'col-resize', zIndex: 30, transition: 'background 0.2s' }}
        onMouseEnter={(e) => (e.currentTarget.style.background = 'rgba(91,191,232,0.4)')}
        onMouseLeave={(e) => (e.currentTarget.style.background = 'transparent')}
      />
    </div>
  );
}
