# FloatPaste 发布流程

发布为 tag 驱动：本地只负责升级版本号和打 tag，检查、构建、产物上传与发布说明骨架全部由 GitHub Actions（`.github/workflows/release.yml`）完成。

## 发布形态

- 安装包：Inno Setup `FloatPaste_版本号_x64-setup.exe`（脚本见 `packaging/floatpaste.iss`）
  - 中文向导；默认安装到 `C:\Program Files\FloatPaste`，需管理员权限
  - 安装时自动卸载检测到的旧版本（兼容 Tauri NSIS 与 MSI），并把指向旧位置的自启动条目迁移到新位置
- 便携版：`FloatPaste-v版本号-windows-x64-portable.zip`（内含 `floatpaste.exe` 与使用说明）
- 校验文件：`SHA256SUMS.txt`，覆盖 Release 全部资产
- 渠道：GitHub Release，先草稿、本机验收后正式发布
- 平台：Windows 10 / Windows 11 x64；默认无代码签名，首次运行可能出现系统警告

## 发布步骤

1. 确认 `main` 上 CI 处于通过状态，本轮功能已全部合入。
2. 升级版本号（三处由脚本统一）：

   ```bash
   node scripts/bump-version.mjs <新版本号>
   ```

3. 提交并打 tag：

   ```bash
   git add -A && git commit -m "chore: 升级版本至 <新版本号>"
   git tag -a v<新版本号> -m "v<新版本号>" && git push --follow-tags
   ```

4. 推送 tag 后 Release 流水线自动执行：
   - 校验 tag 与 `package.json` / `crates/floatpaste-native/Cargo.toml` 版本一致
   - Rust 测试（`floatpaste-core` + `floatpaste-native`）
   - git-cliff 按 Conventional Commits 生成本版变更清单
   - 构建原生壳，编译 Inno 安装包与便携版，创建草稿 Release 并上传产物与 `SHA256SUMS.txt`

   流水线失败时的处理：`gh run view <run-id> --log-failed` 定位原因并修复，删除已推的 tag（`git tag -d v<版本号> && git push origin :refs/tags/v<版本号>`），回退或补提交后从第 3 步重走。

5. 从草稿 Release 下载产物，在本机完成下方"发布前人工验收"。
6. 补充发布说明中的"手动验证建议"段落（模板已带骨架）。
7. 正式发布：

   ```bash
   gh release edit v<新版本号> --draft=false
   ```

任一验收场景失败时，本轮不发布；修复后发下一个版本号，不覆盖原有资产。

## 版本规则

- 遵循 SemVer；内测版本使用 `-beta.N` 等预发布后缀。
- tag 带预发布后缀时，流水线自动勾选 Pre-release。
- 小修复升 patch，新功能/重构升 minor；去掉预发布后缀即为正式版。

## 发布前人工验收

必须直接运行下载下来的产物，而不是只跑开发模式。

必测场景：

1. 启动后能驻留托盘。
2. 复制一段文本后，搜索窗口中能看到新记录。
3. 全局快捷键能唤起速贴。
4. `Up / Down / Enter / Esc / 1..9` 在速贴中正常工作。
5. 在记事本、浏览器输入框、VS Code 中至少各完成一次上屏。
6. 暂停监听后，复制新文本不会继续入库。
7. 关闭搜索窗口后应用仍停留在托盘。

## 发布说明

- 变更清单由 git-cliff 依据提交前缀（`feat:` / `fix:` / `refactor:` 等）自动分组生成，配置见 `cliff.toml`。
- 完整模板见 `docs/release/notes-template.md`，含"已知限制"与"反馈格式"固定段落，`{{version}}` / `{{date}}` / `{{changelog}}` 占位符由流水线渲染。
- 便携包内使用说明模板为 `docs/release/usage-template.md`。
- 发布信息中不标注"熟人内测版"等内部口径。

## 反馈收集

- 用 GitHub Issue 收口，不接受零散聊天记录作为正式结论。
- 问题分类固定为：安装/启动、剪贴监听、速贴唤起、上屏失败、托盘/设置、数据异常。
