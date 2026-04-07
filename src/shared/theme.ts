import { useEffect, useState } from "react";
import type { CustomThemeColors, ThemeMode } from "./types/settings";
import { buildThemeCssVariables, DEFAULT_CUSTOM_THEME_COLORS, sanitizeCustomThemeColors } from "./themeColors";

export const DEFAULT_THEME_MODE: ThemeMode = "system";

function getThemeMediaQuery() {
  if (typeof window === "undefined" || typeof window.matchMedia !== "function") {
    return null;
  }

  return window.matchMedia("(prefers-color-scheme: dark)");
}

export function resolveTheme(themeMode: ThemeMode, prefersDark: boolean): "light" | "dark" {
  if (themeMode === "system") {
    return prefersDark ? "dark" : "light";
  }

  return themeMode;
}

export function useAppliedTheme(
  themeMode: ThemeMode,
  customThemeColors: CustomThemeColors = DEFAULT_CUSTOM_THEME_COLORS,
) {
  const [prefersDark, setPrefersDark] = useState(() => getThemeMediaQuery()?.matches ?? false);

  useEffect(() => {
    const mediaQuery = getThemeMediaQuery();
    if (!mediaQuery) {
      return;
    }

    const handleChange = (event: MediaQueryListEvent) => {
      setPrefersDark(event.matches);
    };

    setPrefersDark(mediaQuery.matches);
    if (typeof mediaQuery.addEventListener === "function") {
      mediaQuery.addEventListener("change", handleChange);
    } else {
      mediaQuery.addListener(handleChange);
    }

    return () => {
      if (typeof mediaQuery.removeEventListener === "function") {
        mediaQuery.removeEventListener("change", handleChange);
      } else {
        mediaQuery.removeListener(handleChange);
      }
    };
  }, []);

  const resolvedTheme = resolveTheme(themeMode, prefersDark);
  const sanitizedColors = sanitizeCustomThemeColors(customThemeColors);

  useEffect(() => {
    const root = document.documentElement;
    root.classList.toggle("dark", resolvedTheme === "dark");
    root.dataset.theme = resolvedTheme;
    root.dataset.themeMode = themeMode;
    root.style.colorScheme = resolvedTheme;
    const themeVars = buildThemeCssVariables(resolvedTheme, sanitizedColors);
    for (const [name, value] of Object.entries(themeVars)) {
      root.style.setProperty(name, value);
    }
  }, [resolvedTheme, sanitizedColors, themeMode]);

  return resolvedTheme;
}
