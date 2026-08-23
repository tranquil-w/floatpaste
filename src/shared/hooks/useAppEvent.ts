import { useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { isTauriRuntime } from "../../bridge/runtime";

/**
 * 订阅 Tauri 应用事件的统一封装，替代各窗口手写的 listen/unlisten 样板。
 *
 * - 浏览器预览（非 Tauri 运行时）不注册监听
 * - handler 通过 ref 保持最新，事件触发时总是调用最近一次渲染的闭包
 * - 卸载竞态：listen 尚未完成即卸载时，注册完成后立刻注销
 */
export function useAppEvent<T = unknown>(
  eventName: string,
  handler: (payload: T) => void | Promise<void>,
  onError?: (error: unknown) => void,
) {
  const handlerRef = useRef(handler);
  const onErrorRef = useRef(onError);

  useEffect(() => {
    handlerRef.current = handler;
    onErrorRef.current = onError;
  });

  useEffect(() => {
    if (!isTauriRuntime()) {
      return;
    }

    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen<T>(eventName, (event) => {
      void handlerRef.current(event.payload);
    })
      .then((cleanup) => {
        if (disposed) {
          cleanup();
          return;
        }
        unlisten = cleanup;
      })
      .catch((error) => {
        if (disposed) {
          return;
        }
        if (onErrorRef.current) {
          onErrorRef.current(error);
        } else {
          console.error(`注册事件监听失败: ${eventName}`, error);
        }
      });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [eventName]);
}
