---
name: release
description: 发布新版本并上传到 GitHub Release。当用户说"发版"、"发布"、"打包上传"、"release"、"升级版本并发布"时使用此 skill。也适用于用户提到版本号升级、构建产物打包、GitHub Release 创建等场景。即使用户没有明确说"release"，只要涉及版本升级+打包+发布的完整流程，都应使用此 skill。
---

# Release 发布流程

发布为 tag 驱动：本地只升级版本号并打 tag，检查、构建、产物上传与发布说明骨架由 GitHub Actions（`.github/workflows/release.yml`）完成。完整规范见 `docs/release/流程.md`。

## 工作流程

### 1. 确定版本号

当前版本见 `package.json`（`bump-version.mjs` 保证四处一致，无需手工核对）。

向用户确认新版本号。如果用户没有指定，根据最近的变更规模建议：

- 小修复/样式调整：patch（如 0.5.1 → 0.5.2）
- 新功能/重构：minor（如 0.5.1 → 0.6.0）
- 内测版：附加 `-beta.N` 后缀（如 0.6.0-beta.1）；去掉后缀即为正式版

### 2. 前置检查

- `main` 上 CI 通过（`gh run list --limit 3`）
- 本轮要发布的功能已全部合入 `main`

### 3. 升级版本号并打 tag

```bash
node scripts/bump-version.mjs <新版本号>
git add -A && git commit -m "chore: 升级版本至 <新版本号>"
git tag -a v<新版本号> -m "v<新版本号>" && git push --follow-tags
```

脚本会统一更新 `package.json`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.lock`，不要手工改版本号。

### 4. 跟踪 Release 流水线

```bash
gh run watch    # 或 gh run list --limit 3 查看 Release 工作流状态
```

流水线内容：版本一致性校验 → lint / 格式检查 / 构建 / 前端与 Rust 测试 → git-cliff 生成本版变更 → 构建安装包与便携版 → 创建草稿 Release（含 `SHA256SUMS.txt`）。

若流水线失败：

1. `gh run view <run-id> --log-failed` 定位原因并修复
2. 删除已推的 tag：`git tag -d v<版本号> && git push origin :refs/tags/v<版本号>`
3. 回退或补提交后重新执行第 3 步

### 5. 本机人工验收

从草稿 Release 下载产物（`gh release download v<版本号> --pattern "*.zip" --pattern "*.exe"`），提醒用户按 `docs/release/流程.md` 的必测场景验收。任一场景失败则本轮不发布。

### 6. 补充发布说明并正式发布

发布说明骨架已由流水线生成（变更清单 + 已知限制 + 反馈格式），需要补"手动验证建议"段落。用编辑后的完整说明文件更新：

```bash
gh release view v<版本号> --json body -q .body > notes.md   # 取当前草稿内容
# 编辑 notes.md，填充"手动验证建议"段落后：
gh release edit v<版本号> --notes-file notes.md --draft=false
```

## 注意事项

- 版本号一律通过 `scripts/bump-version.mjs` 升级，不要手工改文件
- MSI 不支持预发布版本号；tag 带预发布后缀时流水线自动只打 NSIS，无需干预
- 出现阻塞问题后发下一个版本号，不要覆盖原有 Release 资产
- 不要在发布信息中标注"熟人内测版"
