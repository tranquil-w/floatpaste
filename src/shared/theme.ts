import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { ThemeMode } from "./types/settings.ts";
import { isTauriRuntime } from "../bridge/runtime.ts";
import { syncWindowTheme } from "../bridge/commands.ts";
import { getCurrentWindowLabel } from "../bridge/window.ts";
import { deriveSemanticTokens, SHADOW_TOKENS, toCssVariableName } from "./theme/derive.ts";
import {
  ACCENT_FOLLOW_PRESET,
  DEFAULT_THEME_PRESET,
  getThemePreset,
  isThemePresetId,
  nativeWindowTheme,
  resolveAccentHex,
} from "./theme/presets.ts";
import type { ResolvedTheme, SemanticTokens } from "./theme/types.ts";

export {
  ACCENT_FOLLOW_PRESET,
  DEFAULT_THEME_PRESET,
  THEME_PRESETS,
  getThemePreset,
  resolveAccentHex,
} from "./theme/presets.ts";
export { ACCENT_CHOICES } from "./theme/accents.ts";
export type { ResolvedTheme, ThemePresetId } from "./theme/types.ts";

export const DEFAULT_THEME_MODE: ThemeMode = "system";
/** themeAccent 的默认值：跟随所选预设自带的强调色 */
export const DEFAULT_THEME_ACCENT = ACCENT_FOLLOW_PRESET;

/**
 * 最近一次应用的主题选择。设置保存在 Rust 侧（异步），但同一 Tauri 应用的
 * 多窗口共享 localStorage：picker/search 弹出时先同步回放缓存值，
 * 避免等 settings 查询返回期间闪现错误的预设。
 */
const THEME_CACHE_KEY = "floatpaste.themeSelection";

interface ThemeSelection {
  themeMode: ThemeMode;
  themePreset: string;
  themeAccent: string;
}

function sanitizeSelection(selection: ThemeSelection): ThemeSelection {
  const mode: ThemeMode =
    selection.themeMode === "light" || selection.themeMode === "dark"
      ? selection.themeMode
      : "system";
  return {
    themeMode: mode,
    themePreset: isThemePresetId(selection.themePreset)
      ? selection.themePreset
      : DEFAULT_THEME_PRESET,
    themeAccent:
      typeof selection.themeAccent === "string" ? selection.themeAccent : DEFAULT_THEME_ACCENT,
  };
}

function readThemeCache(): ThemeSelection | null {
  try {
    const raw = window.localStorage.getItem(THEME_CACHE_KEY);
    return raw ? sanitizeSelection(JSON.parse(raw) as ThemeSelection) : null;
  } catch {
    return null;
  }
}

function writeThemeCache(selection: ThemeSelection) {
  try {
    window.localStorage.setItem(THEME_CACHE_KEY, JSON.stringify(selection));
  } catch {
    // localStorage 不可用（隐私模式等）只影响首帧回放，主题仍由 settings 驱动
  }
}

function getThemeMediaQuery() {
  if (typeof window === "undefined" || typeof window.matchMedia !== "function") {
    return null;
  }

  return window.matchMedia("(prefers-color-scheme: dark)");
}

export function resolveTheme(themeMode: ThemeMode, prefersDark: boolean): ResolvedTheme {
  if (themeMode === "system") {
    return prefersDark ? "dark" : "light";
  }

  return themeMode;
}

/** 纯函数：给定主题选择与明暗模式，派生全套语义 token（测试与预览共用） */
export function resolveSemanticTokens(
  themePreset: string,
  themeAccent: string,
  resolvedTheme: ResolvedTheme,
): SemanticTokens {
  const preset = getThemePreset(themePreset);
  const accentHex = resolveAccentHex(themeAccent, preset, resolvedTheme);
  return deriveSemanticTokens(preset.scales[resolvedTheme], accentHex, resolvedTheme);
}

/** 把派生结果写入 documentElement 的内联样式（含阴影 token） */
export function applySemanticTokens(tokens: SemanticTokens) {
  const root = document.documentElement;
  for (const [key, value] of Object.entries(tokens)) {
    root.style.setProperty(toCssVariableName(key), value);
  }
  for (const [key, value] of Object.entries(SHADOW_TOKENS)) {
    root.style.setProperty(toCssVariableName(key), value);
  }
}

function applyThemeSelection(selection: ThemeSelection, prefersDark: boolean): ResolvedTheme {
  const resolvedTheme = resolveTheme(selection.themeMode, prefersDark);
  const root = document.documentElement;
  root.classList.toggle("dark", resolvedTheme === "dark");
  root.dataset.theme = resolvedTheme;
  root.dataset.themeMode = selection.themeMode;
  root.dataset.themePreset = selection.themePreset;
  root.style.colorScheme = resolvedTheme;
  applySemanticTokens(
    resolveSemanticTokens(selection.themePreset, selection.themeAccent, resolvedTheme),
  );
  return resolvedTheme;
}

// 模块加载即回放缓存：早于 React 首帧，消除窗口弹出时的预设闪变
const initialCache = typeof window === "undefined" ? null : readThemeCache();
if (initialCache) {
  applyThemeSelection(initialCache, getThemeMediaQuery()?.matches ?? false);
}

export function useAppliedTheme(themeMode: ThemeMode, themePreset: string, themeAccent: string) {
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

  const selection = sanitizeSelection({ themeMode, themePreset, themeAccent });
  const resolvedTheme = resolveTheme(selection.themeMode, prefersDark);

  useEffect(() => {
    const nextSelection = sanitizeSelection({ themeMode, themePreset, themeAccent });
    applyThemeSelection(nextSelection, prefersDark);
    writeThemeCache(nextSelection);
    // 带系统装饰的窗口（设置/编辑器）同步原生标题栏明暗，避免亮标题栏配深色正文。
    // "跟随系统"必须清除窗口级覆盖：WebView2 的 prefers-color-scheme 跟随窗口主题，
    // 一旦显式设过 light/dark，matchMedia 就锁定该值，"跟随系统"会永远解析回旧模式。
    const windowTheme = nativeWindowTheme(nextSelection.themeMode, resolvedTheme);
    if (isTauriRuntime()) {
      // 无边框窗口（速贴/搜索/悬浮提示）的 DWM 边框与圆角跟随窗口 preferred theme，
      // 不同步的话深色窗口在浅色系统上弹出会先闪现一圈浅色边角
      void syncWindowTheme(windowTheme).catch((error) => {
        console.warn("同步窗口原生主题失败", error);
      });
      if (["settings", "editor"].includes(getCurrentWindowLabel())) {
        void getCurrentWindow()
          .setTheme(windowTheme)
          .then(() => {
            if (windowTheme !== null) {
              return;
            }
            // 覆盖清除后 matchMedia 才恢复反映系统真实值，校准一次防止 state 残留
            const mediaQuery = getThemeMediaQuery();
            if (mediaQuery) {
              setPrefersDark(mediaQuery.matches);
            }
          })
          .catch((error) => {
            console.warn("同步窗口原生主题失败", error);
          });
      }
    }
  }, [themeMode, themePreset, themeAccent, prefersDark, resolvedTheme]);

  return resolvedTheme;
}
