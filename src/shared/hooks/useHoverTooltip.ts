import { useEffect, useRef, type MouseEvent } from "react";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { hideTooltip, showTooltip } from "../../bridge/commands";
import { resolveTooltipShowPosition } from "../tooltip/tooltipState";
import { DEFAULT_THEME_ACCENT, DEFAULT_THEME_PRESET, resolveSemanticTokens } from "../theme";
import { SHADOW_TOKENS, toCssVariableName } from "../theme/derive.ts";
import { TOOLTIP_SHOW_DELAY_MS } from "../ui/tooltipConfig";
import type { ClipItemSummary } from "../types/clips";

type UseHoverTooltipOptions = {
  /** 仅 Tauri 运行时启用；浏览器预览保持 no-op */
  enabled: boolean;
  /** 主题预设与强调色，随 tooltip 内容一并传给 tooltip 窗口派生变量 */
  themePreset?: string;
  themeAccent?: string;
  /** 悬停延迟到点后构建 tooltip HTML；数据加载失败时抛错即可放弃本次显示 */
  buildHtml: (item: ClipItemSummary, requestId: number) => Promise<string>;
  /** 可选过滤：返回 false 的条目不触发 tooltip */
  shouldShow?: (item: ClipItemSummary) => boolean;
};

/**
 * 悬停预览 tooltip 的统一调度：延迟触发、请求失效、窗口定位与主题变量
 * 全部收敛在此，调用方只需提供 buildHtml（与可选过滤）。
 */
export function useHoverTooltip(options: UseHoverTooltipOptions) {
  const optionsRef = useRef(options);
  useEffect(() => {
    optionsRef.current = options;
  });

  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const requestIdRef = useRef(0);

  useEffect(() => {
    return () => {
      if (timerRef.current) {
        clearTimeout(timerRef.current);
      }
    };
  }, []);

  /** 取消挂起的显示并隐藏当前 tooltip；会话结束、粘贴、编辑等场景调用 */
  const cancelTooltip = () => {
    if (timerRef.current) {
      clearTimeout(timerRef.current);
      timerRef.current = null;
    }
    requestIdRef.current += 1;
    if (optionsRef.current.enabled) {
      void hideTooltip();
    }
  };

  const handleMouseMove = (event: MouseEvent, item: ClipItemSummary) => {
    const { enabled, shouldShow, buildHtml } = optionsRef.current;
    if (!enabled || (shouldShow && !shouldShow(item))) {
      return;
    }

    requestIdRef.current += 1;
    const requestId = requestIdRef.current;
    const clientPosition = { x: event.clientX, y: event.clientY };
    if (timerRef.current) {
      clearTimeout(timerRef.current);
    }
    timerRef.current = setTimeout(() => {
      timerRef.current = null;
      void (async () => {
        const html = await buildHtml(item, requestId);
        if (requestIdRef.current !== requestId) {
          return;
        }

        const currentWindow = getCurrentWebviewWindow();
        const [outerPosition, scaleFactor] = await Promise.all([
          currentWindow.outerPosition(),
          currentWindow.scaleFactor(),
        ]);
        const position = resolveTooltipShowPosition({
          activeRequestId: requestIdRef.current,
          requestId,
          outerPosition,
          scaleFactor,
          clientPosition,
        });

        if (!position) {
          return;
        }

        const theme = (document.documentElement.dataset.theme as "dark" | "light") ?? "dark";
        // tooltip 是独立窗口，token 以完整 CSS 变量名注入其 :root
        const tokens = resolveSemanticTokens(
          optionsRef.current.themePreset ?? DEFAULT_THEME_PRESET,
          optionsRef.current.themeAccent ?? DEFAULT_THEME_ACCENT,
          theme,
        );
        const themeVars = Object.fromEntries([
          ...Object.entries(tokens).map(([key, value]) => [toCssVariableName(key), value]),
          ...Object.entries(SHADOW_TOKENS).map(([key, value]) => [toCssVariableName(key), value]),
        ]);
        await showTooltip(requestId, position.x, position.y, html, theme, themeVars);
      })().catch((error) => {
        console.warn("[FloatPaste] tooltip 定位或显示失败:", error);
      });
    }, TOOLTIP_SHOW_DELAY_MS);
  };

  return { cancelTooltip, handleMouseMove };
}
