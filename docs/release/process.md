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
4. `scripts/test-release.ps1` 在临时 Windows runner 上解压便携包、静默安装安装包，分别针对交付程序运行同一套真进程 E2E；校验使用说明、安装版本、程序哈希、旧版卸载失败处理、自启迁移、覆盖安装、卸载清理、默认保留/显式清理数据、重解析点目标保护与其他副本隔离。旧版卸载用 `packaging/tests/legacy-uninstaller.cs` 编译的替身注入非零退出码和“退出成功但注册项残留”；不代替真实 NSIS/MSI 的兼容性验收。
5. 自动生成面向用户的发布说明，并保存产物、说明和诊断日志到工作流 Artifact。
6. 创建临时草稿，上传全部资产，重新下载并核对文件集合与 SHA256；全部成功后由工作流公开 Release。

发布 E2E 必须实际执行录入、去重、图片、保留清理、速贴开关与置顶、监听切换、收藏、搜索窗口几何和两段式删除。剪贴板不可用时必须失败，禁止把整个测试跳过计作发布通过。

焦点还原依赖交互桌面；runner 无法取得前台时，工作流必须显示覆盖缺失警告。真实快捷键分发、上屏注入、第三方应用兼容性与视觉效果不由此冒烟测试保证；相关改动按需进行交互桌面验证。测试分层与隔离钩子见 [测试体系](../testing.md)。

安装与卸载验收必须在 GitHub 临时 runner 上执行，禁止在日常开发机上运行 `scripts/test-release.ps1`；验收会操作卸载注册项、自启任务、Run 项与 Win+V 配置，并检查安装和其他路径下同名进程的隔离。

## 发布形态

- 安装包：`FloatPaste_<版本号>_x64-setup.exe`，配置见 `packaging/floatpaste.iss`。
  - 默认安装到 `C:\Program Files\FloatPaste`，需要管理员权限，提供中文许可协议与安装向导。
  - 覆盖安装先经按程序路径命名的退出事件请求同路径运行实例优雅退出（与托盘退出相同的收尾），仍占用时由 Inno Setup 的 Restart Manager 检查待替换文件的占用，不按进程名全局结束实例；完成页启动应用时使用原始用户身份，静默安装不自动启动。
  - 检测 HKCU/HKLM 的 32/64 位卸载视图，发现 Tauri NSIS/MSI 时提供默认勾选的卸载选项。检查卸载器启动结果、退出码与注册项清理；失败则在写入新程序前阻止安装，并记入安装日志。NSIS 优先使用登记的静默命令；MSI 卸载使用 `/norestart`，3010 退出码保留安装器的重启需求，无人值守安装应传入 `/NORESTART`。
  - 安装完成后，将指向当前或已知旧安装位置的当前用户 Run 项和 `FloatPaste for <用户名>` 自启任务迁移到新程序路径，保留参数；任务保留用户、提权级别、触发器与 ACL。同目录覆盖安装不重建任务；不接管其他路径下便携版的自启配置。
  - 确认卸载后，仅结束当前安装路径下的实例，并删除指向该程序的自启任务与当前用户 Run 项。清除当前用户 Win+V 接管的 `V`/`v`，保留其他禁用快捷键；只在配置实际改变且写入成功时提示，生效可能需要重启资源管理器或注销登录，不自动重启资源管理器。
  - 安装始终保留用户数据；首次启动迁移旧目录的规则见 [数据模型](../runtime-behavior.md#数据模型)。交互卸载确认后，在原生卸载向导中显示选项，沿用安装向导的标题区、白色内容区与底部按钮栏；「同时删除历史记录、图片、设置和日志」默认不勾选，单击「卸载」前取消不修改程序或数据。勾选后，在程序卸载完成后清理运行卸载器用户的 `%APPDATA%\floatpaste` 和旧目录 `%APPDATA%\com.floatpaste`，避免下次启动重新迁入遗留数据；删除不可恢复，安装版与便携版共用数据，须先退出其他实例。仅清理这两个固定目录，不遍历其他用户、程序目录中的 `.floatpaste-data` 或临时日志目录，不删除文件条目引用的源文件，也不递归进入目录内重解析点所指向的其他位置；清理不完整时报告剩余目录。
  - 静默卸载默认保留数据，只有显式传入 `/PURGEDATA` 才清理；`/SUPPRESSMSGBOXES` 不代表授权清理。例如：`unins000.exe /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /PURGEDATA`。Run、Win+V 和用户数据清理均作用于运行卸载器的用户；使用其他管理员凭据提权时，不承诺清理原登录用户的这些配置或数据。
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

`release-notes` Environment 的部署分支规则必须新增一条 `v*` 的 **Tag** 规则（不是 Branch 规则），要求仓库所有者批准，并禁止管理员绕过；Key 只存放在此环境中。规则类型选错时，tag 推送会在进入环境前直接失败，报 `Tag "vX.Y.Z" is not allowed to deploy to release-notes`。tag 发布工作流在开始前暂停等待批准，通过后才运行验收并在生成说明时调用模型。PR 和手动演练使用不含 Key 的 `release-preview` 环境，始终采用规则生成的说明。三项配置齐全且存在普通用户可见变化时才调用模型，仅发送筛选后的提交摘要与正文。未配置、请求超时、接口失败或输出格式不符合要求时，使用按规则筛选分组的提交摘要；模型失败须输出工作流警告，不阻断已经通过验收的版本发布。破坏性变化原文不交给模型改写。

## 演练与失败处理

- 在 Actions 的 Release 页面使用 Run workflow，或执行 `gh workflow run release.yml --ref <分支>`，完成相同测试和打包流程；手动运行只保存 Artifact，不创建或公开 Release。
- 修改发布工作流、相关脚本、安装器或 E2E 的 PR 自动执行同样的发布演练；PR 运行只保存 Artifact。
- 失败后用 `gh run view <run-id> --log-failed` 与 Artifact 中的应用日志、安装日志定位原因。
- 临时网络或 runner 环境问题可重跑同一工作流；代码或打包内容需要修复时，修复后升级版本并推送新 tag。
- 上传失败时保留草稿供重跑，工作流只允许更新草稿资产；公开后保持 tag、说明与资产稳定，修复使用新版本号。
- 反馈用 GitHub Issue 收集，附 Windows 版本、目标应用、复现步骤与必要的截图或日志。
