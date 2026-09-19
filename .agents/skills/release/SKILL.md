---
name: release
description: 发布新版本并上传到 GitHub Release。当用户说"发版"、"发布"、"打包上传"、"release"、"升级版本并发布"时使用此 skill。也适用于用户提到版本号升级、构建产物打包、GitHub Release 创建等场景。即使用户没有明确说"release"，只要涉及版本升级+打包+发布的完整流程，都应使用此 skill。
---

# Release 发布

tag 驱动：本地只升级版本号并打 tag，检查、构建、产物上传与发布说明骨架全部由 GitHub Actions（`.github/workflows/release.yml`）完成。

## 发布形态

- Inno 安装包 `FloatPaste_<版本>_x64-setup.exe`（中文向导，默认 Program Files，安装时自动卸载旧版并迁移自启动）+ 便携版 zip（内含 exe 与使用说明）+ `SHA256SUMS.txt`
- 渠道：GitHub Release，先草稿、本机验收后转正式
- 平台：Windows 10 / 11 x64；无代码签名，首启可能有系统警告

## 版本规则

- SemVer：小修复升 patch，新功能/重构升 minor；内测版加 `-beta.N` 后缀，去掉后缀即正式版
- tag 带预发布后缀时，流水线自动勾选 Pre-release

## 执行步骤

1. **确定版本号**：当前版本见 `package.json`；向用户确认，未指定时按变更规模建议。

2. **前置检查**：`gh run list --limit 3` 确认 `main` 上 CI 通过，本轮功能已全部合入。

3. **升级版本号并打 tag**：

   ```bash
   node scripts/bump-version.mjs <新版本号>
   git add -A && git commit -m "chore: 升级版本至 <新版本号>"
   git tag -a v<新版本号> -m "v<新版本号>" && git push --follow-tags
   ```

   脚本统一更新 `package.json`、`crates/floatpaste-native/Cargo.toml`、根 `Cargo.lock`，不手工改版本号。

4. **跟踪流水线**：`gh run watch`。流水线内容：版本一致性校验 → Rust 测试 → git-cliff 生成本版变更清单 → 构建 Inno 安装包与便携版 → 创建草稿 Release（含 `SHA256SUMS.txt`）。

   失败恢复：`gh run view <run-id> --log-failed` 定位修复 → 删已推 tag（`git tag -d v<版本号> && git push origin :refs/tags/v<版本号>`）→ 从第 3 步重走。

5. **本机人工验收**：`gh release download v<版本号> --pattern "*.zip" --pattern "*.exe"` 下载产物，提醒用户**直接运行下载的产物**（不是开发模式）按必测场景验收：

   1. 启动后驻留托盘
   2. 复制文本后搜索窗口出现新记录
   3. 全局快捷键唤起速贴
   4. `Up / Down / Enter / Esc / 1..9` 在速贴中正常
   5. 记事本、浏览器输入框、VS Code 各完成一次上屏
   6. 暂停监听后复制不再入库
   7. 关闭搜索窗口后仍驻留托盘

   任一场景失败本轮不发布；修复后发下一个版本号，不覆盖已有 Release 资产。

6. **补发布说明并正式发布**（骨架已由流水线生成，补"手动验证建议"段落）：

   ```bash
   gh release view v<版本号> --json body -q .body > .artifacts/release-notes-draft.md
   # 编辑 .artifacts/release-notes-draft.md 补段落后：
   gh release edit v<版本号> --notes-file .artifacts/release-notes-draft.md --draft=false
   ```

## 注意事项

- 版本号一律经 `scripts/bump-version.mjs`，不手工改文件
- 发布信息中不标注"熟人内测版"等内部口径
- 发布规范变更时，同一提交内同步更新 `docs/release/process.md`（人工阅读版规范，内容与本文件保持一致）
