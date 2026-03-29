import type { MouseEvent } from "react";
import { startCurrentWindowResize, type WindowResizeDirection } from "../../bridge/window";

export type WindowResizeHandle = {
  key: string;
  direction: WindowResizeDirection;
  className: string;
};

type WindowResizeHandlesProps = {
  handles: WindowResizeHandle[];
  errorLabel: string;
  beforeResizeStart?: () => Promise<void> | void;
};

export function WindowResizeHandles({
  handles,
  errorLabel,
  beforeResizeStart,
}: WindowResizeHandlesProps) {
  async function handleResizeStart(
    direction: WindowResizeDirection,
    event: MouseEvent<HTMLDivElement>,
  ) {
    event.preventDefault();
    event.stopPropagation();

    try {
      await beforeResizeStart?.();
      await startCurrentWindowResize(direction);
    } catch (error) {
      console.warn(`启动${errorLabel}窗口拉伸失败`, error);
    }
  }

  return (
    <>
      {handles.map((handle) => (
        <div
          key={handle.key}
          aria-hidden="true"
          className={handle.className}
          onMouseDown={(event) => {
            void handleResizeStart(handle.direction, event);
          }}
        />
      ))}
    </>
  );
}
