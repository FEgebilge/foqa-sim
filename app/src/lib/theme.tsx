import { useLayoutEffect, useState, type ReactNode } from "react";
import { ThemeContext } from "./context";
import { applyPalette, type Theme } from "./palette";

const KEY = "foqa-sim.theme";
const stored = (): Theme => {
  try {
    return localStorage.getItem(KEY) === "light" ? "light" : "dark";
  } catch {
    return "dark";
  }
};

/** Dark (night cockpit) by default; the choice is remembered on this machine. */
export function ThemeProvider({ children }: { children: ReactNode }) {
  const [theme, setThemeState] = useState<Theme>(stored);
  useLayoutEffect(() => {
    document.documentElement.dataset.theme = theme;
    applyPalette(theme);
  }, [theme]);
  const setTheme = (next: Theme) => {
    setThemeState(next);
    try {
      localStorage.setItem(KEY, next);
    } catch {
      // Storage can be unavailable; the theme still applies for this session.
    }
  };
  return <ThemeContext value={{ theme, setTheme }}>{children}</ThemeContext>;
}
