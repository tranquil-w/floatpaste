---
name: release
description: 发布新版本并上传到 GitHub Release。当用户说"发版"、"发布"、"打包上传"、"release"、"升级版本并发布"时使用此 skill。也适用于用户提到版本号升级、构建产物打包、GitHub Release 创建等场景。即使用户没有明确说"release"，只要涉及版本升级+打包+发布的完整流程，都应使用此 skill。
---

# Release 发布流程

自动化版本升级、构建、打包和 GitHub Release 发布。

## 工作流程

### 1. 确定版本号

检查当前版本：

```bash
# package.json 和 tauri.conf.json 和 Cargo.toml 中的 version 字段
grep '"version"' package.json
grep '^version' src-tauri/Cargo.toml
grep '"version"' src-tauri/tauri.conf.json
```

向用户确认新版本号。如果用户没有指定，根据最近的变更规模建议：
- 小修复/样式调整：patch（如 0.2.0-beta.1 → 0.2.0-beta.2）
- 新功能/重构：minor（如 0.1.0-beta.3 → 0.2.0-beta.1）
- 正式版：去掉 pre-release 标识（如 0.2.0-beta.1 → 0.2.0）

### 2. 更新版本号

需要同步修改三个文件中的版本号：

```
package.json            → "version": "<新版本>"
src-tauri/Cargo.toml    → version = "<新版本>"
src-tauri/tauri.conf.json → "version": "<新版本>"
```

### 3. 构建并打包

```bash
pnpm tauri build
```

MSI 打包器不支持 pre-release 标识（如 `beta.1`）。如果版本号包含 pre-release：
- 打包时 MSI 会失败，这是预期行为
- exe 已经编译好在 `src-tauri/target/release/floatpaste.exe`
- 用 PowerShell 压缩为 zip：

```bash
cd src-tauri/target/release && powershell -Command "Compress-Archive -Path floatpaste.exe -DestinationPath FloatPaste-<版本>-windows-x64.zip -Force"
```

### 4. 提交版本变更并打 tag

```bash
git add package.json src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/tauri.conf.json
git commit -m "chore: 升级版本至 <版本>"
git tag v<版本>
```

注意：`Cargo.lock` 也会因 Cargo.toml 版本变更而更新，必须一起提交。

### 5. 推送

```bash
git push && git push origin v<版本>
```

### 6. 生成 Release Notes 并创建 GitHub Release

**Release Notes 必须基于自上次发版以来的全部变更**，不能只看最近几次提交。

获取变更范围：

```bash
gh release list --limit 5          # 找到上一个 release tag
git log <上个tag>..HEAD --oneline  # 全部提交
gh pr list --state merged           # 已合并的 PR（含详细变更说明）
```

#### Release Notes 格式

```markdown
# FloatPaste v<版本> 发布说明

发布日期：<YYYY-MM-DD>

## 本次更新

### 新功能
- **功能名**：描述

### 改进
- **改进项**：描述

### 修复
- 修复描述

## 手动验证建议

1. 验证步骤
2. 验证步骤

## 已知限制

- 当前只支持文本剪贴项
- 不同桌面应用对 `Ctrl+V` 注入的兼容性仍在收口
- 若系统缺少 WebView2 运行时，程序可能无法正常启动

## 反馈格式

- 目标应用：
- 操作步骤：
- 实际结果：
- 预期结果：
- 是否稳定复现：
- 附件：
  - 录屏 / 截图
  - 日志片段
```

#### 创建 Release

```bash
gh release create v<版本> \
  <zip文件路径> \
  --title "v<版本>" \
  --notes "<Release Notes 内容>"
```

如果需要更新已有的 Release Notes：

```bash
gh release edit v<版本> --notes "<新内容>"
```

## 注意事项

- 版本号三处必须保持一致：`package.json`、`Cargo.toml`、`tauri.conf.json`
- `Cargo.lock` 必须随 `Cargo.toml` 一起提交
- MSI 不支持 pre-release 版本号，纯数字版本才能打 MSI 安装包
- Release Notes 要全面覆盖自上次发版以来的所有变更，从 PR 描述中提取用户可见变化
- 不要在发布信息中标注"熟人内测版"
