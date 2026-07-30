import { useEffect, useRef, useState } from "react";
import type { ClipItemSummary } from "../types/clips";
import { getImageUrl } from "../../bridge/imageUrl";

type ImageUrlCache = Map<string, string | null>;

/**
 * 管理剪辑项图片 URL 的解析缓存。
 *
 * - 自动对传入 `items` 中的图片项预取 URL（失败则缓存为 null 以便渲染降级）。
 * - `getCached` 同步读取缓存供 JSX 渲染使用。
 * - `resolve` 异步解析单个项（tooltip 等延迟场景），命中缓存直接返回。
 * - `markError` 在 `<img onError>` 时标记失败（带幂等保护）。
 * - `version` 在缓存变化时自增，驱动组件重渲染。
 *
 * 内部以 ref 持有可变缓存、以 version state 触发刷新，避免每次解析都引发重渲染。
 */
export function useImageUrlCache(items: ClipItemSummary[]) {
  const cacheRef = useRef<ImageUrlCache>(new Map());
  const pendingRef = useRef<Set<string>>(new Set());
  const [version, bumpVersion] = useState(0);

  useEffect(() => {
    let disposed = false;
    const pending = pendingRef.current;
    const cache = cacheRef.current;

    for (const item of items) {
      if (item.type !== "image" || !item.imagePath || cache.has(item.id) || pending.has(item.id)) {
        continue;
      }

      pending.add(item.id);
      void getImageUrl(item.imagePath)
        .then((imageUrl) => {
          cache.set(item.id, imageUrl);
        })
        .catch(() => {
          cache.set(item.id, null);
        })
        .finally(() => {
          pending.delete(item.id);
          if (!disposed) {
            bumpVersion((current) => current + 1);
          }
        });
    }

    return () => {
      disposed = true;
    };
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
      bumpVersion((current) => current + 1);
      return imageUrl;
    } catch {
      cacheRef.current.set(item.id, null);
      bumpVersion((current) => current + 1);
      return null;
    }
  };

  const markError = (id: string) => {
    if (cacheRef.current.get(id) === null) {
      return;
    }
    cacheRef.current.set(id, null);
    bumpVersion((current) => current + 1);
  };

  return { getCached, resolve, markError, version };
}
