import { useCallback, useEffect, useRef, useState } from "react";

/**
 * 两段式危险操作确认：首次对同一目标进入待确认态（配合 UI 提示），
 * 超时或调用 reset 自动撤销；限时内再次对同一目标触发才执行 onConfirm。
 */
export function useArmedConfirm<T>(
  onConfirm: (target: T) => void | Promise<void>,
  timeoutMs = 3000,
) {
  const [armedTarget, setArmedTarget] = useState<T | null>(null);
  const armedTargetRef = useRef<T | null>(null);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const onConfirmRef = useRef(onConfirm);

  useEffect(() => {
    onConfirmRef.current = onConfirm;
  });

  const reset = useCallback(() => {
    if (timerRef.current) {
      clearTimeout(timerRef.current);
      timerRef.current = null;
    }
    armedTargetRef.current = null;
    setArmedTarget(null);
  }, []);

  useEffect(() => {
    return () => {
      if (timerRef.current) {
        clearTimeout(timerRef.current);
      }
    };
  }, []);

  const request = useCallback(
    (target: T) => {
      if (armedTargetRef.current !== target) {
        if (timerRef.current) {
          clearTimeout(timerRef.current);
        }
        armedTargetRef.current = target;
        setArmedTarget(target);
        timerRef.current = setTimeout(reset, timeoutMs);
        return;
      }

      reset();
      void onConfirmRef.current(target);
    },
    [reset, timeoutMs],
  );

  return { armedTarget, request, reset };
}
