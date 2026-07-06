import { useEffect, useRef } from 'react';
import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import '@xterm/xterm/css/xterm.css';

interface Props {
  height: number;
  onClose: () => void;
  onResizeStart: (e: React.MouseEvent) => void;
  /** Bumped when the surrounding layout changes so the terminal re-fits. */
  fitKey: number;
}

export function TerminalPanel({ height, onClose, onResizeStart, fitKey }: Props) {
  const hostRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const wsRef = useRef<WebSocket | null>(null);

  // Create the terminal + socket once.
  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;

    const term = new Terminal({
      fontSize: 12,
      fontFamily: 'ui-monospace, "SF Mono", Menlo, monospace',
      cursorBlink: true,
      theme: { background: '#1a1a1a', foreground: '#cccccc', cursor: '#cccccc' },
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(host);
    fit.fit();
    termRef.current = term;
    fitRef.current = fit;

    const proto = location.protocol === 'https:' ? 'wss' : 'ws';
    const ws = new WebSocket(`${proto}://${location.host}/api/terminal`);
    ws.binaryType = 'arraybuffer';
    wsRef.current = ws;

    ws.onopen = () => {
      ws.send(JSON.stringify({ r: [term.cols, term.rows] }));
      term.focus();
    };
    ws.onmessage = (ev) => {
      if (ev.data instanceof ArrayBuffer) term.write(new Uint8Array(ev.data));
      else term.write(ev.data as string);
    };
    ws.onclose = () => term.write('\r\n\x1b[90m[terminal đã đóng]\x1b[0m\r\n');

    const dataSub = term.onData((d) => {
      if (ws.readyState === WebSocket.OPEN) ws.send(JSON.stringify({ d }));
    });
    const resizeSub = term.onResize(({ cols, rows }) => {
      if (ws.readyState === WebSocket.OPEN) ws.send(JSON.stringify({ r: [cols, rows] }));
    });

    const onWinResize = () => fit.fit();
    window.addEventListener('resize', onWinResize);

    return () => {
      window.removeEventListener('resize', onWinResize);
      dataSub.dispose();
      resizeSub.dispose();
      ws.close();
      term.dispose();
    };
  }, []);

  // Re-fit when the panel height or layout changes.
  useEffect(() => {
    const id = window.setTimeout(() => fitRef.current?.fit(), 60);
    return () => window.clearTimeout(id);
  }, [height, fitKey]);

  return (
    <div className="terminal-panel" style={{ height }}>
      <div className="terminal-resizer" onMouseDown={onResizeStart} />
      <div className="terminal-head">
        <span>⌘ TERMINAL</span>
        <button onClick={onClose} title="Đóng terminal">✕</button>
      </div>
      <div className="terminal-host" ref={hostRef} />
    </div>
  );
}
