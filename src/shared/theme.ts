import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { CustomThemeColors, ThemeMode } from "./types/settings";
import { isTauriRuntime } from "../bridge/runtime";
import { getCurrentWindowLabel } from "../bridge/window";
import {
  buildThemeCssVariables,
  DEFAULT_CUSTOM_THEME_COLORS,
  sanitizeCustomThemeColors,
} from "./themeColors";

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
    // 带系统装饰的窗口（设置/编辑器）同步原生标题栏明暗，避免亮标题栏配深色正文
    if (isTauriRuntime() && ["settings", "editor"].includes(getCurrentWindowLabel())) {
      void getCurrentWindow()
        .setTheme(resolvedTheme)
        .catch((error) => {
          console.warn("同步窗口原生主题失败", error);
        });
    }
  }, [resolvedTheme, sanitizedColors, themeMode]);

  return resolvedTheme;
}
