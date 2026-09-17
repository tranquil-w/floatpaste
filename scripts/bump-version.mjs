#!/usr/bin/env node
// 统一升级版本号，保持三处一致：
//   package.json / crates/floatpaste-native/Cargo.toml / Cargo.lock（工作区根）
// 用法: node scripts/bump-version.mjs <新版本>   例如 0.9.0 或 0.9.0-beta.1

import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const files = {
  packageJson: join(repoRoot, "package.json"),
  cargoToml: join(repoRoot, "crates", "floatpaste-native", "Cargo.toml"),
  cargoLock: join(repoRoot, "Cargo.lock"),
};

const semverPattern = /^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/;
const newVersion = process.argv[2];

if (!newVersion || !semverPattern.test(newVersion)) {
  console.error(`用法: node scripts/bump-version.mjs <新版本>，例如 0.9.0`);
  process.exit(1);
}

function readText(path) {
  return readFileSync(path, "utf8");
}

function assertChanged(before, after, label) {
  if (before === after) {
    console.error(`错误：未能更新 ${label} 中的版本号，请检查文件结构。`);
    process.exit(1);
  }
}

function replaceJsonVersion(path, label) {
  const before = readText(path);
  // 只替换顶层第一个 "version" 字段，避免 JSON.stringify 重排产生无关格式改动
  const after = before.replace(/("version"\s*:\s*")[^"]*(")/, `$1${newVersion}$2`);
  assertChanged(before, after, label);
  const previous = before.match(/"version"\s*:\s*"([^"]*)"/)[1];
  if (previous === newVersion) {
    console.error(`错误：${label} 已是 ${newVersion}，无需升级。`);
    process.exit(1);
  }
  writeFileSync(path, after, "utf8");
  return previous;
}

const packageVersion = replaceJsonVersion(files.packageJson, "package.json");

const cargoTomlBefore = readText(files.cargoToml);
// [package] 段的 version 是文件中第一个行首 version 赋值
const cargoTomlAfter = cargoTomlBefore.replace(/^version\s*=\s*"[^"]*"/m, `version = "${newVersion}"`);
assertChanged(cargoTomlBefore, cargoTomlAfter, "Cargo.toml");
writeFileSync(files.cargoToml, cargoTomlAfter, "utf8");

const cargoLockBefore = readText(files.cargoLock);
const cargoLockAfter = cargoLockBefore.replace(
  /(name = "floatpaste-native"\r?\nversion = )"[^"]*"/,
  `$1"${newVersion}"`,
);
assertChanged(cargoLockBefore, cargoLockAfter, "Cargo.lock");
writeFileSync(files.cargoLock, cargoLockAfter, "utf8");

console.log(`版本已升级: ${packageVersion} -> ${newVersion}`);
console.log("已更新: package.json, crates/floatpaste-native/Cargo.toml, Cargo.lock");
console.log(`后续步骤:\n  git add -A && git commit -m "chore: 升级版本至 ${newVersion}"\n  git tag -a v${newVersion} -m "v${newVersion}" && git push --follow-tags`);
