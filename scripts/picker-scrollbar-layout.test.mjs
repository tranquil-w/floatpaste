import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const source = await readFile(new URL("../src/features/picker/PickerShell.tsx", import.meta.url), "utf8");

assert.match(
  source,
  /scrollbar-gutter:stable_both-edges/,
  "picker 列表滚动容器缺少稳定双侧滚动槽，滚动条显隐时会造成内容宽度跳动或左右留白失衡",
);

assert.doesNotMatch(
  source,
  /grid gap-1 px-0\.5 transition-colors/,
  "picker 列表内容容器仍然保留额外横向内边距，列表项左右边距不够紧凑",
);

assert.doesNotMatch(
  source,
  /rounded-b-md bg-pg-canvas-subtle px-0\.5 py-1\.5/,
  "picker 列表外层容器横向内边距还没有在内容之外进一步收窄",
);

assert.match(
  source,
  /rounded-b-md bg-pg-canvas-subtle px-0 py-1\.5/,
  "picker 列表外层容器横向内边距还没有收紧到 px-0",
);

assert.match(
  source,
  /rounded-\[8px\] px-1\.5 py-2/,
  "picker 列表项内容横向内边距还没有调整到 px-1.5",
);
