import { ConfigProvider, theme as antdTheme, type ThemeConfig } from "antd";
import { createContext, useContext, useEffect, useMemo, useState, type PropsWithChildren } from "react";

export type ThemeMode = "dark" | "light";

const STORAGE_KEY = "flowkit:theme-mode";

type ThemeCtx = {
  themeMode: ThemeMode;
  setThemeMode: (mode: ThemeMode) => void;
  toggleTheme: () => void;
};

const Ctx = createContext<ThemeCtx | null>(null);

function readInitialTheme(): ThemeMode {
  const saved = localStorage.getItem(STORAGE_KEY);
  if (saved === "dark" || saved === "light") return saved;
  return "dark";
}

export function ThemeProvider({ children }: PropsWithChildren) {
  const [themeMode, setThemeMode] = useState<ThemeMode>(() => readInitialTheme());

  useEffect(() => {
    localStorage.setItem(STORAGE_KEY, themeMode);
    document.documentElement.setAttribute("data-theme", themeMode);
  }, [themeMode]);

  const config = useMemo<ThemeConfig>(
    () => ({
      algorithm:
        themeMode === "dark"
          ? antdTheme.darkAlgorithm
          : antdTheme.defaultAlgorithm,
      token: {
        colorPrimary: "#5b8def",
        borderRadius: 10,
      },
    }),
    [themeMode]
  );

  const value = useMemo<ThemeCtx>(
    () => ({
      themeMode,
      setThemeMode,
      toggleTheme: () =>
        setThemeMode((m) => (m === "dark" ? "light" : "dark")),
    }),
    [themeMode]
  );

  return (
    <Ctx.Provider value={value}>
      <ConfigProvider theme={config}>{children}</ConfigProvider>
    </Ctx.Provider>
  );
}

export function useThemeMode() {
  const v = useContext(Ctx);
  if (!v) throw new Error("useThemeMode must be used inside ThemeProvider");
  return v;
}

