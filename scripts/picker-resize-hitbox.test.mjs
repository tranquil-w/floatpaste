import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const source = await readFile(new URL("../src/features/picker/PickerShell.tsx", import.meta.url), "utf8");

assert.doesNotMatch(
  source,
  /absolute inset-y-3 left-0 z-20 w-2 cursor-ew-resize/,
  "picker 左侧调整宽度热区仍然过宽，会提前劫持靠近边框的鼠标命中",
);

assert.doesNotMatch(
  source,
  /absolute inset-y-3 right-0 z-20 w-2 cursor-ew-resize/,
  "picker 右侧调整宽度热区仍然过宽，会在鼠标移到滚动条时过早显示横向拉伸光标",
);

assert.match(
  source,
  /absolute inset-y-3 left-0 z-20 w-px cursor-ew-resize/,
  "picker 左侧调整宽度热区还没有收窄到 1px 边框宽度",
);

assert.match(
  source,
  /absolute inset-y-3 right-0 z-20 w-px cursor-ew-resize/,
  "picker 右侧调整宽度热区还没有收窄到 1px 边框宽度",
);
