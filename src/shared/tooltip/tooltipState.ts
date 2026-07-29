const DEFAULT_TOOLTIP_OFFSET = {
  x: 12,
  y: 16,
};

export function resolveTooltipShowPosition({
  activeRequestId,
  requestId,
  outerPosition,
  scaleFactor,
  clientPosition,
}: {
  activeRequestId: number;
  requestId: number;
  outerPosition: { x: number; y: number };
  scaleFactor: number;
  clientPosition: { x: number; y: number };
}): { x: number; y: number } | null {
  if (activeRequestId !== requestId) {
    return null;
  }

  return {
    x: outerPosition.x + (clientPosition.x + DEFAULT_TOOLTIP_OFFSET.x) * scaleFactor,
    y: outerPosition.y + (clientPosition.y + DEFAULT_TOOLTIP_OFFSET.y) * scaleFactor,
  };
}
