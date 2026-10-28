import assert from "node:assert/strict";
import { spawnSync, execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const script = fileURLToPath(new URL("./check.ps1", import.meta.url));

// 只替换外部命令，在真实 Git 临时仓库和 PowerShell 子进程验证调度与失败语义。
function fixture(t) {
  // Windows runner 的 TEMP 可能采用 8.3 短路径，统一真实路径后核对目录恢复。
  const root = realpathSync.native(mkdtempSync(join(tmpdir(), "floatpaste-check-")));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const repo = join(root, "repo");
  const bin = join(root, "bin");
  mkdirSync(join(repo, "scripts"), { recursive: true });
  mkdirSync(bin);
  copyFileSync(script, join(repo, "scripts/check.ps1"));
  writeFileSync(join(repo, ".gitignore"), ".artifacts/\n");
  writeFileSync(join(repo, "tracked.txt"), "original\n");
  writeFileSync(join(repo, "未跟踪 中文.txt"), "snapshot includes UTF-8 filenames\n");
  const git = (...args) => execFileSync("git", args, { cwd: repo, stdio: "pipe" }).toString().trim();
  git("init", "-q");
  git("add", "--", ".gitignore", "tracked.txt", "scripts/check.ps1");
  git("-c", "user.name=Check Test", "-c", "user.email=check@example.invalid", "commit", "-qm", "fixture");
  const stub = join(root, "stub.mjs");
  writeFileSync(stub, `
import { appendFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
const [program, ...args] = process.argv.slice(2);
const phase = args.includes('l2_headless_ui_contracts') ? 'ui' : args[0];
appendFileSync(process.env.CHECK_CALLS, JSON.stringify({ program, args,
  cwd: process.cwd(), strict: process.env.FLOATPASTE_E2E_STRICT,
  keep: process.env.FLOATPASTE_E2E_KEEP, scale: process.env.SLINT_SCALE_FACTOR ?? null,
  snapshots: process.env.FLOATPASTE_UI_SNAPSHOTS ?? null,
  exe: process.env.FLOATPASTE_E2E_EXE ?? null }) + '\\n');
if (program === 'node' && process.env.CHECK_MUTATE) {
  const name = process.env.CHECK_MUTATE === 'untracked' ? 'new.txt' : 'tracked.txt';
  writeFileSync(join(process.cwd(), name), 'changed during validation');
}
if (program === 'node' && process.env.CHECK_WARNING) console.log('::warning::焦点还原未覆盖');
if (phase !== process.env.CHECK_SILENT) console.log(program + ' ' + args.join(' '));
if (phase === process.env.CHECK_FAIL) { console.error('fixture failure evidence'); process.exit(23); }
`);
  for (const name of ["cargo", "node"]) {
    writeFileSync(join(bin, `${name}.cmd`), `@"%CHECK_NODE%" "%CHECK_STUB%" ${name} %*\r\n@exit /b %errorlevel%\r\n`);
  }
  const wrapper = join(root, "wrapper.ps1");
  writeFileSync(wrapper, `
$originalEncoding = [Console]::OutputEncoding.CodePage
& $env:CHECK_SCRIPT @args
$code = $LASTEXITCODE
@{ cwd = (Get-Location).Path; strict = $env:FLOATPASTE_E2E_STRICT; keep = $env:FLOATPASTE_E2E_KEEP;
   snapshots = $env:FLOATPASTE_UI_SNAPSHOTS; scale = $env:SLINT_SCALE_FACTOR; exe = $env:FLOATPASTE_E2E_EXE;
   encoding_restored = ([Console]::OutputEncoding.CodePage -eq $originalEncoding) } |
    ConvertTo-Json | Set-Content -LiteralPath $env:CHECK_RESTORED
exit $code
`);
  function run(args = [], extraEnv = {}) {
    const callsPath = join(root, "calls.jsonl");
    const restoredPath = join(root, "restored.json");
    // Windows 环境变量名不区分大小写，避免同时传入 PATH 和 Path。
    const env = Object.fromEntries(Object.entries(process.env).filter(([key]) => key.toLowerCase() !== "path"));
    Object.assign(env, {
      PATH: `${bin};${process.env.PATH}`, CHECK_NODE: process.execPath,
      CHECK_STUB: stub, CHECK_SCRIPT: join(repo, "scripts/check.ps1"),
      CHECK_CALLS: callsPath, CHECK_RESTORED: restoredPath,
      FLOATPASTE_E2E_STRICT: "original-strict", FLOATPASTE_E2E_KEEP: "original-keep",
      FLOATPASTE_UI_SNAPSHOTS: "original-snapshots", SLINT_SCALE_FACTOR: "2", FLOATPASTE_E2E_EXE: "old-release.exe",
      CHECK_FAIL: "", CHECK_MUTATE: "", CHECK_WARNING: "", CHECK_SILENT: "", ...extraEnv,
    });
    const result = spawnSync("pwsh", ["-NoProfile", "-File", wrapper, ...args], {
      cwd: root, env, encoding: "utf8", timeout: 30_000, windowsHide: true,
    });
    assert.ifError(result.error);
    const reports = readdirSync(join(repo, ".artifacts/checks"));
    assert.equal(reports.length, 1, result.stdout + result.stderr);
    const reportDir = join(repo, ".artifacts/checks", reports[0]);
    const report = JSON.parse(readFileSync(join(reportDir, "result.json"), "utf8").replace(/^\uFEFF/, ""));
    const calls = readFileSync(callsPath, "utf8").trim().split("\n").map(JSON.parse);
    const restored = JSON.parse(readFileSync(restoredPath, "utf8").replace(/^\uFEFF/, ""));
    const { encoding_restored, ...restoredEnv } = restored;
    assert.equal(encoding_restored, true);
    assert.deepEqual(restoredEnv, {
      cwd: root, strict: "original-strict", keep: "original-keep",
      snapshots: "original-snapshots", scale: "2", exe: "old-release.exe",
    });
    return { ...result, report, calls, reportDir };
  }
  return { run, repo, git };
}

test("完整入口串行执行现有门槛、绑定源码并恢复调用环境", (t) => {
  const f = fixture(t);
  const { status, report, calls, reportDir } = f.run([], { CHECK_SILENT: "fmt" });
  assert.equal(status, 0);
  assert.equal(report.status, "passed");
  assert.equal(report.source_changed, false);
  assert.equal(report.source_start.head, f.git("rev-parse", "HEAD"));
  assert.match(report.source_start.content_sha256, /^[A-F0-9]{64}$/);
  assert.deepEqual(report.steps.map((s) => s.name), ["scripts", "fmt", "rust-tests", "native-build"]);
  assert.ok(report.steps.every((s) => s.status === "passed" && s.exit_code === 0));
  assert.equal(readFileSync(join(reportDir, "fmt.log"), "utf8"), "");
  assert.deepEqual(calls.map((c) => c.args[0]), ["--test", "fmt", "test", "build"]);
  assert.ok(calls.every((c) => c.cwd === f.repo && c.strict === "1" && c.keep === "1" && c.scale === null && c.snapshots === null && c.exe === null));
  assert.ok(calls.filter((c) => ["test", "build"].includes(c.args[0])).every((c) => c.args.includes("--locked")));
});

test("失败保留原始退出码与日志，后续步骤保持未运行", (t) => {
  const { status, report, calls, reportDir } = fixture(t).run([], { CHECK_FAIL: "test" });
  assert.notEqual(status, 0);
  assert.equal(report.status, "failed");
  assert.deepEqual(report.steps.map((s) => s.status), ["passed", "passed", "failed", "not_run"]);
  assert.equal(report.steps[2].exit_code, 23);
  assert.equal(calls.length, 3);
  assert.match(readFileSync(join(reportDir, report.steps[2].log), "utf8"), /fixture failure evidence/);
  assert.equal(report.source_changed, false);
});

for (const mutation of ["tracked", "untracked"]) {
  test(`执行期间${mutation === "tracked" ? "已有" : "新增未跟踪"}源码变化不得计为通过`, (t) => {
    const { status, report } = fixture(t).run(["-Scope", "Scripts"], { CHECK_MUTATE: mutation });
    assert.notEqual(status, 0);
    assert.equal(report.status, "failed");
    assert.equal(report.source_changed, true);
    assert.notEqual(report.source_start.content_sha256, report.source_end.content_sha256);
    assert.match(report.error, /源码状态发生变化/);
    assert.equal(report.steps[0].status, "passed");
  });
}

test("Rust 与截图范围按需选择，截图三档经 Cargo 逐次执行", (t) => {
  const { status, report, calls } = fixture(t).run(["-Scope", "Rust", "-UiSnapshots"]);
  assert.equal(status, 0);
  assert.equal(report.scope, "Rust");
  assert.equal(report.ui_snapshots, true);
  assert.ok(calls.every((c) => c.program === "cargo"));
  const ui = calls.filter((c) => c.args.includes("l2_headless_ui_contracts"));
  assert.deepEqual(ui.map((c) => c.scale), ["1", "1.25", "1.5"]);
  assert.ok(ui.every((c) => c.snapshots === "1" && c.args.includes("--locked")));
});

test("脚本范围不冒充完整验收，焦点覆盖缺失进入报告", (t) => {
  const { status, report, calls } = fixture(t).run(["-Scope", "Scripts"], { CHECK_WARNING: "1" });
  assert.equal(status, 0);
  assert.equal(report.scope, "Scripts");
  assert.equal(report.steps.length, 1);
  assert.equal(calls[0].program, "node");
  assert.deepEqual(report.coverage_notes, ["[scripts] ::warning::焦点还原未覆盖"]);
});
