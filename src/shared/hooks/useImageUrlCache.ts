import { useEffect, useRef, useState } from "react";
import type { ClipItemSummary } from "../types/clips";
import { getImageUrl } from "../../bridge/imageUrl";

type ImageUrlCache = Map<string, string | null>;

/** 同时进行的图片解析 IPC 上限：大列表一次性发起上百个并发请求会拖垮 IPC 通道 */
const MAX_CONCURRENT_RESOLUTIONS = 4;

/**
 * 管理剪辑项图片 URL 的解析缓存。
 *
 * - 列表图片按并发上限排队预取（失败则缓存为 null 以便渲染降级）。
 * - 同一轮微任务内完成的多个解析合并为一次重渲染，避免逐张图触发全列表 re-render。
 * - `getCached` 同步读取缓存供 JSX 渲染使用。
 * - `resolve` 异步解析单个项（tooltip 等即时场景，不排队），命中缓存直接返回。
 * - `markError` 在 `<img onError>` 时标记失败（带幂等保护）。
 * - `version` 在缓存变化时自增，驱动组件重渲染。
 */
export function useImageUrlCache(items: ClipItemSummary[]) {
  const cacheRef = useRef<ImageUrlCache>(new Map());
  const pendingRef = useRef<Set<string>>(new Set());
  const queueRef = useRef<Array<() => void>>([]);
  const activeCountRef = useRef(0);
  const flushScheduledRef = useRef(false);
  const mountedRef = useRef(true);
  const [version, bumpVersion] = useState(0);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      queueRef.current = [];
    };
  }, []);

  const scheduleFlush = () => {
    if (flushScheduledRef.current || !mountedRef.current) {
      return;
    }
    flushScheduledRef.current = true;
    queueMicrotask(() => {
      flushScheduledRef.current = false;
      if (mountedRef.current) {
        bumpVersion((current) => current + 1);
      }
    });
  };

  const pumpQueue = () => {
    while (activeCountRef.current < MAX_CONCURRENT_RESOLUTIONS) {
      const start = queueRef.current.shift();
      if (!start) {
        return;
      }
      start();
    }
  };

  const enqueueResolve = (item: ClipItemSummary) => {
    pendingRef.current.add(item.id);
    queueRef.current.push(() => {
      activeCountRef.current += 1;
      void getImageUrl(item.imagePath as string)
        .then((imageUrl) => {
          cacheRef.current.set(item.id, imageUrl);
        })
        .catch(() => {
          cacheRef.current.set(item.id, null);
        })
        .finally(() => {
          pendingRef.current.delete(item.id);
          activeCountRef.current -= 1;
          scheduleFlush();
          pumpQueue();
        });
    });
  };

  useEffect(() => {
    for (const item of items) {
      if (
        item.type !== "image" ||
        !item.imagePath ||
        cacheRef.current.has(item.id) ||
        pendingRef.current.has(item.id)
      ) {
        continue;
      }

      enqueueResolve(item);
    }
    pumpQueue();
  }, [items]);

  const getCached = (id: string): string | null => cacheRef.current.get(id) ?? null;

  const resolve = async (item: ClipItemSummary): Promise<string | null> => {
    if (item.type !== "image" || !item.imagePath) {
      return null;
    }

    const cached = cacheRef.current.get(item.id);
    if (cached !== undefined) {
      return cached;
    }

    try {
      const imageUrl = await getImageUrl(item.imagePath);
      cacheRef.current.set(item.id, imageUrl);
      scheduleFlush();
      return imageUrl;
    } catch {
      cacheRef.current.set(item.id, null);
      scheduleFlush();
      return null;
    }
  };

  const markError = (id: string) => {
    if (cacheRef.current.get(id) === null) {
      return;
    }
    cacheRef.current.set(id, null);
    scheduleFlush();
  };

  return { getCached, resolve, markError, version };
}
