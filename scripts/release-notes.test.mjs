import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { compareVersions, selectBase, userChanges, fallbackNotes, rewriteNotes } from "./generate-release-notes.mjs";

const release = (tag_name, extra = {}) => ({ tag_name, draft: false, prerelease: tag_name.includes("-"), ...extra });
const commit = (subject, body = "") => ({ subject, body });

test("版本顺序区分正式版、数字预发布编号和不同主版本", () => {
  assert.ok(compareVersions("v0.10.0-beta.10", "0.10.0-beta.9") > 0);
  assert.ok(compareVersions("0.10.0", "0.10.0-beta.10") > 0);
  assert.ok(compareVersions("0.10.0-beta.1", "0.9.9") > 0);
  assert.ok(compareVersions("0.10.0-beta", "0.10.0-beta.1") < 0);
  assert.equal(compareVersions("v0.10.0", "0.10.0"), 0);
});

test("Beta 比较上一公开 Beta，跳过草稿、当前版本、未来版本与非祖先", () => {
  const releases = [release("v0.10.0-beta.6"), release("v0.10.0-beta.5"),
    release("v0.10.0-beta.4", { draft: true }), release("v0.10.0-beta.3"),
    release("v0.10.0-beta.2"), release("v0.9.0")];
  assert.equal(selectBase(releases, "0.10.0-beta.5", (tag) => tag !== "v0.10.0-beta.3"), "v0.10.0-beta.2");
  assert.equal(selectBase(releases, "0.10.0-beta.5", () => true), "v0.10.0-beta.3");
});

test("正式版汇总整个 Beta 周期，首次公开发布没有基线", () => {
  const releases = [release("v0.10.0-beta.5"), release("v0.9.0"), release("v0.8.0")];
  assert.equal(selectBase(releases, "0.10.0", () => true), "v0.9.0");
  assert.equal(selectBase(releases, "0.8.0", () => true), null);
});

test("内部提交被过滤，重复摘要被合并，破坏性变化保留迁移详情", () => {
  const changes = userChanges([
    commit("feat(picker): 增加收藏入口"), commit("feat: 增加收藏入口"),
    commit("test: 增加测试"), commit("docs: 整理文档"), commit("ci: 升级 runner"),
    commit("refactor: 整理模块"), commit("chore!: 调整配置格式", "BREAKING CHANGE: 配置文件需转换\n请先备份。"),
    commit("fix: 修复窗口定位"),
  ]);
  assert.equal(changes.length, 3);
  const notes = fallbackNotes(changes);
  assert.match(notes, /## 升级注意\n\n- 调整配置格式：配置文件需转换\n  请先备份。/);
  assert.doesNotMatch(notes, /测试|文档|runner|整理模块/);
  assert.doesNotMatch(fallbackNotes([]), /##/);
});

test("未配置模型直接使用筛选结果，无网络依赖", async () => {
  const changes = userChanges([commit("fix: 修复窗口定位")]);
  assert.equal(await rewriteNotes(changes, {}, () => assert.fail("不得发起请求")), fallbackNotes(changes));
});

test("模型失败或返回空白、英文、错误格式时回退，不丢失变更", async (t) => {
  const warning = t.mock.method(console, "warn", () => {});
  const changes = userChanges([commit("fix: 修复窗口定位")]);
  const config = { key: "test", url: "https://example.invalid/chat/completions", model: "test" };
  for (const request of [
    async () => { throw new Error("连接失败"); },
    async () => ({ ok: false, status: 429 }),
    ...["", "## Fixes\n- Fixed position", "```\n## 修复\n- 定位\n```"].map((content) => async () => ({ ok: true, json: async () => ({ choices: [{ message: { content } }] }) })),
  ]) {
    assert.equal(await rewriteNotes(changes, config, request), fallbackNotes(changes));
  }
  assert.equal(warning.mock.callCount(), 5);
});

test("模型只改写普通变化，升级注意保留原文", async () => {
  const changes = userChanges([commit("fix: 修复窗口定位"), commit("feat!: 改变数据格式", "BREAKING CHANGE: 升级前备份数据")]);
  const notes = await rewriteNotes(changes, { key: "test", url: "https://example.invalid", model: "test" }, async (_url, options) => {
    assert.doesNotMatch(JSON.stringify(JSON.parse(options.body).messages[1]), /改变数据格式/);
    return { ok: true, json: async () => ({ choices: [{ message: { content: "## 修复\n\n- 搜索窗口的位置更准确。" } }] }) };
  });
  assert.match(notes, /升级前备份数据/);
  assert.match(notes, /搜索窗口的位置更准确/);
});

test("模板中的下载文件名与发布产物一致，变更中的替换字符保持原文", () => {
  const dir = mkdtempSync(join(tmpdir(), "floatpaste-notes-"));
  try {
    const changes = join(dir, "changes.md"), output = join(dir, "notes.md");
    writeFileSync(changes, "## 修复\n\n- 保留 $& 和 $'。\n", "utf8");
    execFileSync(process.execPath, ["scripts/render-release-notes.mjs", "0.10.0-beta.5", "2026-10-18", changes, output]);
    const notes = readFileSync(output, "utf8");
    assert.match(notes, /FloatPaste_0\.10\.0-beta\.5_x64-setup\.exe/);
    assert.match(notes, /FloatPaste-v0\.10\.0-beta\.5-windows-x64-portable\.zip/);
    assert.ok(notes.includes("保留 $& 和 $'。"));
    assert.doesNotMatch(notes, /\{\{|手动验证建议|反馈格式/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
