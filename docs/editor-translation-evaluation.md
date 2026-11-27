# 编辑窗口选区翻译评估

对应 [Issue #40](https://github.com/tranquil-w/floatpaste/issues/40)。现行产品约束是剪贴内容留在本机。若后续实现，优先验证离线英译简中。远程翻译需要先明确产品隐私承诺如何调整。本页的远程演示只使用固定公开样句。 [架构概览](architecture.md#产品约束)

## 服务与语言范围

| 来源 | 语言范围与验证结论 |
| --- | --- |
| Argos Translate（本机） | 候选首发方向为英语 `en` → 中文 `zh`。官方索引也提供反向模型。实施前需在目标 Windows 环境预装模型和分句资源，断网验证是否能启动和翻译，并确认简体输出、质量及模型再分发许可。 [项目说明](https://github.com/argosopentech/argos-translate) · [模型索引](https://github.com/argosopentech/argospm-index/blob/main/index.json) |
| Azure Translator（远程） | 官方文本翻译覆盖英语 `en` 和简体中文 `zh-Hans`，请求需要认证。官方文档称文本翻译不持久保存提交内容，正文仍会发送到服务端。本次核对官方文档，未用凭据调用。 [语言支持](https://learn.microsoft.com/en-us/azure/ai-services/translator/language-support) · [服务及认证](https://learn.microsoft.com/en-us/azure/ai-services/translator/text-translation/overview) · [数据处理](https://learn.microsoft.com/en-us/azure/ai-foundry/responsible-ai/translator/data-privacy-security) |
| MyMemory（远程演示） | 实测英语 `en` → 简体中文 `zh-CN`，`Good morning!` 返回 `早上好！`。查询正文出现在 GET URL，接口限制单段 500 UTF-8 字节。服务条款披露对提交片段的收集及与机器翻译合作方的数据共享，因此只用于公开样句。 [接口说明](https://mymemory.translated.net/doc/spec.php) · [服务条款](https://mymemory.translated.net/terms-and-conditions) |

## 拟议交互

1. 文本条目选中非空白正文后才允许开启翻译。打开时记录条目、草稿版本及选区范围，原样捕获选区，默认目标为简体中文。编辑器选区按 UTF-8 字节处理。 [编辑窗口规格](editor-window.md#编辑区命中与自管滚动)
2. 远程来源显示服务名称、目标语言、内容将离开本机的提示以及完整的待发送选区。只有点击「发送选区」才发请求。正文载荷只包含这段文本。标题、标签、HTML、选区外的文字和剪贴板数据均不发送。关闭、Esc 或取消结束流程，重试需重新确认发送。
3. 翻译成功时同时预览原选区和译文。点击「替换选区」后才更新草稿，沿用现有手动保存语义。等待和预览期间可取消。关闭编辑器、切换条目、草稿或选区变化后丢弃旧结果，防止写入其他位置。失败时提示原因，保留草稿及选区。 [保存与关闭](editor-window.md#工作区与常用操作)

## 可复现演示

在 PowerShell 7 中执行下方代码。模拟草稿有三行，第二行是固定选区。演示读取控制台输入，不读取系统剪贴板或历史库。输入 `y` 才发送公开样句或确认替换，直接回车取消。`-SimulateFailure` 模拟服务不可用，不联网。

```powershell
function Show-TranslationDemo([switch]$SimulateFailure) {
    $draft = @('上一行保留', 'Good morning!', '下一行保留')
    $selection = 1
    $source = $draft[$selection]
    Write-Host ($draft -join "`n")
    Write-Host "MyMemory (api.mymemory.translated.net) · 英语 → 简体中文"
    Write-Host "将发送到外部服务的完整选区（第 2 行）`n$source"
    if ((Read-Host '发送选区？[y/N]') -eq 'y') {
        try {
            if ($SimulateFailure) { throw '模拟服务不可用' }
            $query = [uri]::EscapeDataString($source)
            $result = Invoke-RestMethod -ErrorAction Stop -TimeoutSec 15 -MaximumRedirection 0 `
                -Uri "https://api.mymemory.translated.net/get?q=$query&langpair=en%7Czh-CN"
            $translation = [string]$result.responseData.translatedText
            if ($result.responseStatus -ne 200 -or [string]::IsNullOrWhiteSpace($translation)) {
                throw '服务未返回可用译文'
            }
            Write-Host "原选区`n$source`n译文预览`n$translation"
            if ((Read-Host '替换选区？[y/N]') -eq 'y') {
                $draft[$selection] = $translation
            }
        } catch {
            Write-Host '翻译失败，已保留原文。可重新运行演示。'
        }
    }
    Write-Host '最终草稿'
    Write-Output $draft
}

Show-TranslationDemo
```

| 操作 | 预期 |
| --- | --- |
| 发送时回车 | 不发请求，三行均保留。 |
| 发送输入 `y`，预览后回车 | 请求只有公开样句，显示译文预览，三行均保留。 |
| 两次都输入 `y` | 第二行替换为预览中的译文，第一和第三行保留。 |
| 运行 `Show-TranslationDemo -SimulateFailure` 并确认发送 | 显示失败提示，不出现替换提示，三行均保留。 |

演示覆盖发送范围、两次确认、取消和失败。实际 Slint 选区、编辑期间的过期结果、撤销和保存留待桌面功能实现时验证。
