import { createContext, useContext, type ReactNode } from "react";
import type { Theme } from "./palette";

// App-wide contexts; their providers are ThemeProvider (lib/theme.tsx) and NoticeProvider (components/notices.tsx).

export const ThemeContext = createContext<{ theme: Theme; setTheme: (theme: Theme) => void }>({ theme: "dark", setTheme: () => {} });
export const useTheme = () => useContext(ThemeContext);

export interface Notice {
  title: string;
  body?: ReactNode;
  tone?: "info" | "error";
  action?: { label: string; run: () => void };
}
export const NoticeContext = createContext<(notice: Notice) => void>(() => {});
export const useNotify = () => useContext(NoticeContext);
