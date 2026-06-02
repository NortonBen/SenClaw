import { useEffect, useRef } from 'react';
import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import { AttachAddon } from '@xterm/addon-attach';
import '@xterm/xterm/css/xterm.css';
import type { Host } from './types';

interface TerminalViewProps {
  host: Host;
}

export function TerminalView({ host }: TerminalViewProps) {
  const terminalRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);

  useEffect(() => {
    if (!terminalRef.current) return;

    const term = new Terminal({
      theme: {
        background: '#0f172a',
        foreground: '#e2e8f0',
        cursor: '#38bdf8',
        selectionBackground: 'rgba(56, 189, 248, 0.3)',
        black: '#0f172a',
        red: '#ef4444',
        green: '#22c55e',
        yellow: '#eab308',
        blue: '#3b82f6',
        magenta: '#d946ef',
        cyan: '#06b6d4',
        white: '#f8fafc',
      },
      fontFamily: 'Menlo, Monaco, "Courier New", monospace',
      fontSize: 14,
      lineHeight: 1.5,
      cursorBlink: true,
      allowTransparency: true,
    });
    
    const fitAddon = new FitAddon();
    term.loadAddon(fitAddon);
    
    term.open(terminalRef.current);
    fitAddon.fit();
    termRef.current = term;

    term.writeln(`\x1b[36mConnecting to ${host.user}@${host.host}...\x1b[0m`);

    // We assume the app is running in a browser with a standard origin.
    // If the API proxy strips this, we can use relative paths or determine the correct base.
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    
    // Construct the websocket URL based on the current window location to ensure it works through the space proxy
    // In semaclaw spaces, requests go to `/api/space/apps/ssh-manager/proxy/`
    // We just replace `http` with `ws` and append `api/ws/terminal/:id`
    
    // The current path could be `/api/space/apps/ssh-manager/proxy/`
    // We need to resolve `api/ws/terminal/:id` relative to this.
    // fetch('./api/...') resolves correctly. For WS, we need an absolute URL.
    const baseUrl = new URL('./api', window.location.href);
    baseUrl.protocol = protocol;
    
    const wsUrl = `${baseUrl.href}/ws/terminal/${host.id}`;
    const socket = new WebSocket(wsUrl);

    socket.onopen = () => {
      term.writeln('\x1b[32mConnected.\x1b[0m');
      const attachAddon = new AttachAddon(socket);
      term.loadAddon(attachAddon);
    };

    socket.onerror = () => {
      term.writeln('\r\n\x1b[31mWebSocket Connection Error\x1b[0m');
    };

    socket.onclose = (e) => {
      term.writeln(`\r\n\x1b[33mConnection closed (code: ${e.code})\x1b[0m`);
    };

    const handleResize = () => fitAddon.fit();
    window.addEventListener('resize', handleResize);

    return () => {
      window.removeEventListener('resize', handleResize);
      socket.close();
      term.dispose();
    };
  }, [host]);

  return (
    <div style={{ 
      width: '100%', 
      height: '100%', 
      overflow: 'hidden', 
      backgroundColor: '#0f172a', 
    }}>
      <div ref={terminalRef} style={{ width: '100%', height: '100%', padding: '8px' }} />
    </div>
  );
}
