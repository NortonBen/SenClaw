import { createContext, useContext } from 'react';
import type { WsHook } from '../hooks/useWebSocket';

interface AppContextValue {
  ws: WsHook;
  isDarkMode: boolean;
  toggleTheme: () => void;
}

export const AppContext = createContext<AppContextValue>(null!);

export function useAppContext() {
  return useContext(AppContext);
}
