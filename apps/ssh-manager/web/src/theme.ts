import { createContext, useContext } from 'react';

/** Dark / light mode. Mirrors the contract deepwiki uses (see apps/deepwiki). */
export type Mode = 'dark' | 'light';

/** Semantic colors used across the SSH Manager UI, resolved per mode. */
export interface Palette {
  /** Outermost / content background (was #111827). */
  layoutBg: string;
  /** Sider / header / card / modal surface (was #1f2937). */
  containerBg: string;
  /** Form input background (was #111827). */
  inputBg: string;
  /** Hover / selected / active-tab surface (was #374151). */
  elevated: string;
  /** Hairline borders (was #374151). */
  border: string;
  /** Primary text (was #fff / #f9fafb). */
  text: string;
  /** Muted / secondary text (was #9ca3af). */
  textMuted: string;
  /** Embedded terminal background — kept dark in both modes (terminals read better dark). */
  terminalBg: string;
  /** Logs console background — kept dark in both modes (colored log text needs a dark surface). */
  consoleBg: string;
  /** Logs console default text. */
  consoleText: string;
}

export const PALETTES: Record<Mode, Palette> = {
  dark: {
    layoutBg: '#111827',
    containerBg: '#1f2937',
    inputBg: '#111827',
    elevated: '#374151',
    border: '#374151',
    text: '#f9fafb',
    textMuted: '#9ca3af',
    terminalBg: '#0f172a',
    consoleBg: '#0b1220',
    consoleText: '#e5e7eb',
  },
  light: {
    layoutBg: '#f3f4f6',
    containerBg: '#ffffff',
    inputBg: '#ffffff',
    elevated: '#eef2f7',
    border: '#e5e7eb',
    text: '#111827',
    textMuted: '#6b7280',
    terminalBg: '#0f172a',
    consoleBg: '#0b1220',
    consoleText: '#e5e7eb',
  },
};

/** Resolve the initial mode: saved preference → system preference → dark. */
export function detectInitialMode(): Mode {
  try {
    const saved = localStorage.getItem('ssh-mode');
    if (saved === 'dark' || saved === 'light') return saved;
  } catch { /* ignore */ }
  if (typeof window !== 'undefined' && window.matchMedia?.('(prefers-color-scheme: dark)').matches) {
    return 'dark';
  }
  return 'light';
}

export interface ThemeCtx {
  mode: Mode;
  isDark: boolean;
  palette: Palette;
}

export const ThemeContext = createContext<ThemeCtx>({
  mode: 'dark',
  isDark: true,
  palette: PALETTES.dark,
});

/** Access the active mode + resolved palette from any component. */
export const useAppTheme = () => useContext(ThemeContext);
