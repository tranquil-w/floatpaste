#!/usr/bin/env node
// 渲染发布说明：notes-template.md + 筛选后的变更 -> 完整 Markdown
// CI 发版与本地预览共用。
// 用法: node scripts/render-release-notes.mjs <版本号> <日期YYYY-MM-DD> <变更路径> [输出路径]
// 不指定输出路径时打印到 stdout。

import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const argv = process.argv.slice(2);
const templateIndex = argv.indexOf("--template");
const templateOverride = templateIndex >= 0 ? argv.splice(templateIndex, 2)[1] : null;
const [version, date, changesPath, outputPath] = argv;

if (!version || !date || !changesPath) {
  console.error(
    "用法: node scripts/render-release-notes.mjs <版本号> <日期YYYY-MM-DD> <变更路径|-> [输出路径] [--template <模板路径>]",
  );
  process.exit(1);
}

const normalize = (text) => text.replace(/\r\n?/g, "\n").trim();

const template = normalize(
  readFileSync(join(repoRoot, templateOverride ?? "docs/release/notes-template.md"), "utf8"),
);
// 传 "-" 时表示没有变更清单（例如渲染便携包内的使用说明）。
const changelog = changesPath === "-" ? "" : normalize(readFileSync(changesPath, "utf8"));
const repository = process.env.GITHUB_REPOSITORY ?? "tranquil-w/floatpaste";

const body = template
  .replace(/\{\{version\}\}/g, `v${version.replace(/^v/, "")}`)
  .replace(/\{\{version_number\}\}/g, version.replace(/^v/, ""))
  .replace(/\{\{date\}\}/g, date)
  .replace(/\{\{repository\}\}/g, repository)
  .replace(/\{\{changelog\}\}/g, () => changelog);

if (outputPath) {
  writeFileSync(outputPath, `${body}\n`, "utf8");
  console.log(`发布说明已写入: ${outputPath}`);
} else {
  console.log(body);
}
