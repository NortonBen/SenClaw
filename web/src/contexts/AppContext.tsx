import { createContext, useContext } from 'react';
import type { WsHook } from '../hooks/useWebSocket';

interface AppContextValue {
  ws: WsHook;
  isDarkMode: boolean;
  toggleTheme: () => void;
  /** Compact chat mode for the desktop app menu-bar window (?embed=1). Hides global nav. */
  embed: boolean;
}

export const AppContext = createContext<AppContextValue>(null!);

export function useAppContext() {
  return useContext(AppContext);
}
