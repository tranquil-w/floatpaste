# FloatPaste 发布流程

推送版本 tag 后，由 GitHub Actions 完成测试、构建、交付产物验收、发布说明生成与公开发布；发布过程无需本地 agent 或固定人工草稿验收。

## 发布入口

1. 把本轮改动合入 `main`，确认 CI 通过。
2. 用 `node scripts/bump-version.mjs <新版本号>` 同步 `package.json`、native `Cargo.toml` 与 `Cargo.lock`。
3. 只提交本轮版本文件，确认目标提交包含在 `main` 中，再创建并推送 tag：

   ```bash
   git add package.json crates/floatpaste-native/Cargo.toml Cargo.lock
   git commit -m "chore: 升级版本至 <新版本号>"
   git tag -a v<新版本号> -m "v<新版本号>"
   git push origin main
   git push origin v<新版本号>
   ```

4. 用 `gh run list --workflow release.yml` 与 `gh run watch <run-id>` 查看结果；以工作流成功且对应 Release 已公开作为发布完成依据。

遵循 SemVer：修复升 patch，新功能升 minor；测试版使用 `-beta.N` 等后缀。带后缀的 tag 自动发布为 Pre-release，不设为 Latest；正式版本说明须覆盖自上一公开正式版以来的变化。

## 自动发布门禁

工作流入口为 `.github/workflows/release.yml`，同一仓库的发布串行执行：

1. 校验 tag、仓库版本一致且提交属于 `main`；禁止覆盖已经公开的 Release。
2. 执行发布说明脚本检查与完整 Rust 测试，使用严格 E2E 模式。
3. 构建 `target/release/floatpaste.exe`，编译安装包，生成便携包及 SHA256 校验文件。
4. `scripts/test-release.ps1` 在临时 Windows runner 上解压便携包、静默安装安装包，分别针对交付程序运行同一套真进程 E2E；校验使用说明、安装版本、程序哈希与卸载清理。
5. 自动生成面向用户的发布说明，并保存产物、说明和诊断日志到工作流 Artifact。
6. 创建临时草稿，上传全部资产，重新下载并核对文件集合与 SHA256；全部成功后由工作流公开 Release。

发布 E2E 必须实际执行录入、去重、图片、保留清理、速贴开关与置顶、监听切换、收藏、搜索窗口几何和两段式删除。剪贴板不可用时必须失败，禁止把整个测试跳过计作发布通过。

焦点还原依赖交互桌面；runner 无法取得前台时，工作流必须显示覆盖缺失警告。真实快捷键分发、上屏注入、第三方应用兼容性与视觉效果不由此冒烟测试保证；相关改动按需进行交互桌面验证。测试分层与隔离钩子见 [测试体系](../testing.md)。

安装与卸载验收必须在 GitHub 临时 runner 上执行，禁止在日常开发机上运行 `scripts/test-release.ps1`；安装器会操作卸载注册项和结束同名进程。

## 发布形态

- 安装包：`FloatPaste_<版本号>_x64-setup.exe`，配置见 `packaging/floatpaste.iss`。
  - 默认安装到 `C:\Program Files\FloatPaste`，需要管理员权限，提供中文许可协议与安装向导。
  - 检测到旧版时提供默认勾选的卸载选项，兼容 Tauri NSIS/MSI；迁移指向旧安装位置的自启动条目。
- 便携版：`FloatPaste-v<版本号>-windows-x64-portable.zip`，包含程序与使用说明。
- `SHA256SUMS.txt`：覆盖安装包与便携包。
- 平台：Windows 10 / 11 x64；程序未签名，首次运行可能出现系统风险提示。

## 发布说明

发布说明由 `scripts/generate-release-notes.mjs` 生成，模板为 `notes-template.md`；便携包使用说明模板为 `usage-template.md`。

- 测试版相对上一公开版本比较；正式版相对上一公开正式版比较。跳过草稿、当前版本、未来版本及不属于当前提交祖先链的 Release。
- 没有符合条件的公开版本时，从当前提交的完整历史提取变化。
- 按新增、改进、修复、升级注意组织用户可见变化，合并重复摘要，不展示空章节；普通内部重构、文档、测试、CI 与依赖维护提交不进入正文。
- 破坏性变化必须在提交中使用 `!` 或 `BREAKING CHANGE:` 标记，并写清影响与迁移要求；生成说明时保留原文。
- 正文说明用户能感知的行为变化，提供对应范围的完整变更链接、下载入口和 Issue 反馈入口；不放内部验收清单或空反馈表。
- 发布信息中不使用“熟人内测版”等内部口径。

可选模型改写使用 OpenAI Chat Completions 兼容的标准 API，在仓库 Actions 配置中设置：

| 类型 | 名称 | 内容 |
|------|------|------|
| `release-notes` Environment Secret | `RELEASE_NOTES_API_KEY` | 标准 API 的访问密钥 |
| Variable | `RELEASE_NOTES_API_URL` | 完整 HTTPS Chat Completions 接口 URL |
| Variable | `RELEASE_NOTES_MODEL` | 该接口支持的模型标识 |

`release-notes` Environment 仅允许 `v*` tag 部署，要求仓库所有者批准，并禁止管理员绕过；Key 只存放在此环境中。tag 发布工作流在开始前暂停等待批准，通过后才运行验收并在生成说明时调用模型。PR 和手动演练使用不含 Key 的 `release-preview` 环境，始终采用规则生成的说明。三项配置齐全且存在普通用户可见变化时才调用模型，仅发送筛选后的提交摘要与正文。未配置、请求超时、接口失败或输出格式不符合要求时，使用按规则筛选分组的提交摘要；模型失败须输出工作流警告，不阻断已经通过验收的版本发布。破坏性变化原文不交给模型改写。

## 演练与失败处理

- 在 Actions 的 Release 页面使用 Run workflow，或执行 `gh workflow run release.yml --ref <分支>`，完成相同测试和打包流程；手动运行只保存 Artifact，不创建或公开 Release。
- 修改发布工作流、相关脚本、安装器或 E2E 的 PR 自动执行同样的发布演练；PR 运行只保存 Artifact。
- 失败后用 `gh run view <run-id> --log-failed` 与 Artifact 中的应用日志、安装日志定位原因。
- 临时网络或 runner 环境问题可重跑同一工作流；代码或打包内容需要修复时，修复后升级版本并推送新 tag。
- 上传失败时保留草稿供重跑，工作流只允许更新草稿资产；公开后保持 tag、说明与资产稳定，修复使用新版本号。
- 反馈用 GitHub Issue 收集，附 Windows 版本、目标应用、复现步骤与必要的截图或日志。
