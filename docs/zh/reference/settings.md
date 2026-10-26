# 设置项

本页列出所有设置项、在应用中的位置，以及它们在 `settings.json` 中的键名。通常在应用里修改即可；只有自动化部署或恢复时才需要手工编辑文件。

## 设置保存位置 {#where-settings-are-stored}

| 安装方式 | 设置文件夹 |
| --- | --- |
| Scoop | `scoop\persist\meetcue\config`（升级后保留） |
| 便携 zip | `MeetCue.exe` 同级的 `config` |
| 上述文件夹不可写时的回退位置 | `%APPDATA%\MeetCue` |

文件夹中有 `settings.json`、`latency-history.json`、`usage.jsonl`、`logs\` 和 `practice\`。**设置 › 诊断 › 关于** 会显示当前使用的文件夹。API 密钥不保存在这里，而是在 Windows 凭据管理器中。

如果 `settings.json` 无法读取，MeetCue 会把它重命名为 `settings.json.bad` 并以默认设置启动。

## 通用

| 设置 | 键名 | 默认值 |
| --- | --- | --- |
| 界面语言 | `general.uiLanguage`：`system`、`en`、`zh-CN` | `system` |
| 提示语言 | `general.hintLanguage`：`followQuestion`、`en`、`zh-CN` | `followQuestion` |
| 强调色 | `general.accent` | `#F5B841` |
| 登录系统时启动 MeetCue | `general.autostart` | `false` |
| 共享屏幕时隐藏面板 | `overlay.hideInScreenShare` | `true` |
| 鼠标穿透面板 | `overlay.clickThrough` | `false` |
| 面板形态 | `overlay.mode`：`full`、`mini`、`hidden` | `full` |
| 面板位置（拖动面板移动；**重置面板位置** 可放回原处） | `overlay.x`、`overlay.y` | 右上角 |
| 重新运行设置向导 | `general.wizardCompleted` | 完成设置前为 `false` |

所有设置修改后立即生效，会议进行中也一样：模型、密钥、语言和提示设置从下一个问题起生效，语音识别和音频采集无需重启会话即可切换。

## 音频

| 设置 | 键名 | 默认值 |
| --- | --- | --- |
| 麦克风（我） | `audio.micDevice`；留空表示系统默认 | 空 |
| 与会者声音 | `audio.remoteSource`：`allSystemSound` | `allSystemSound` |
| 会议语言 | `audio.languages`，例如 `["zh", "en"]` | `["zh", "en"]` |
| 人声检测门限 | `audio.vadThresholdDb`，单位 dBFS，−70 到 −10 | `-45` |
| 静音多久算一句结束 | `audio.endpointSilenceMs`，500 到 3000 毫秒 | `700` |

## 供应商与费用

| 设置 | 键名 |
| --- | --- |
| 语音识别 | `providers.asr` |
| 问题检测模型 | `providers.detect` |
| 要点模型 | `providers.hints` |
| 各供应商的接口地址（OpenAI 兼容接口必填，Anthropic 可选） | `providers.baseUrls`，例如 `{"openai-compatible": "https://host/v1"}`。旧文件中的 `providers.openaiBaseUrl` 会在加载时移到这里 |
| HTTPS 代理 | `providers.proxy`；留空表示使用系统代理 |
| 手动填写的价格（列在 **你填写的价格** 中，可在那里删除） | `providers.pricingOverrides` |
| 费用估算假设（**费用如何估算**） | `providers.estimate`：`speechRatio`、`questionsPerHour`、`hintInputTokens`、`hintOutputTokens`、`detectInputTokens`、`detectOutputTokens`、`systemTokens`、`cancelOverhead` |
| 预算 | `providers.budget.sessionLimitUsd`、`monthlyLimitUsd`、`onLimit`（`notifyOnly`、`pauseHints`、`endSession`） |

`asr`、`detect`、`hints` 各自包含 `provider`、`model` 和 `endpoint`。端点为 `{"mode": "auto"}`（实测最快）或 `{"mode": "host", "host": "<主机名>"}`。

## 提示

| 设置 | 键名 | 默认值 |
| --- | --- | --- |
| 我的角色 | `hints.role` | 空 |
| 会前资料 | `hints.materials`（名称、路径和抽取的文字） | 无 |
| 只提示指向我的问题 | `hints.onlyDirectedToMe` | `false` |
| 插话自动排队 | `hints.queueInterruptions` | `true` |
| 跟踪已讲要点 | `hints.trackCoverage` | `true` |
| 提前触发 | `hints.earlyTrigger` | `true` |

## 快捷键

| 设置 | 键名 | 默认值 |
| --- | --- | --- |
| 开始 / 暂停拾取 | `hotkeys.toggleCapture` | `Ctrl+Alt+Space` |
| 切换面板形态 | `hotkeys.cyclePanel` | `Ctrl+Alt+H` |
| 上一问 | `hotkeys.focusPrev` | `Ctrl+Alt+[` |
| 下一问 | `hotkeys.focusNext` | `Ctrl+Alt+]` |
| 标为已答 | `hotkeys.markAnswered` | `Ctrl+Alt+Enter` |
| 面板上移 | `hotkeys.moveUp` | `Ctrl+Alt+Shift+Up` |
| 面板下移 | `hotkeys.moveDown` | `Ctrl+Alt+Shift+Down` |
| 面板左移 | `hotkeys.moveLeft` | `Ctrl+Alt+Shift+Left` |
| 面板右移 | `hotkeys.moveRight` | `Ctrl+Alt+Shift+Right` |

## 练习

| 设置 | 键名 | 默认值 |
| --- | --- | --- |
| 问题语音转文字 | `practice.playback`：`realPipeline`（开）、`simulatedLatency`（关） | `realPipeline` |
| 提示难度 | `practice.hintLevel`：`full`、`headsOnly`、`delayed`、`off` | `full` |
| 延迟显示的秒数 | `practice.hintDelayS` | `5` |
| 停顿多久算答完（秒） | `practice.answerPauseS` | `3` |
| 未开口提醒（秒） | `practice.nudgeAfterS` | `20` |
| 追问概率 | `practice.followUpProbability` | `0.3` |
| 朗读语速 | `practice.speechRate` | `1.0` |
| 我的回答语音转文字 | `practice.recognizeMe` | `true` |
| 英文语音 | `practice.voiceEn`：Windows 语音 id，留空表示第一个 | `""` |
| 中文语音 | `practice.voiceZh`：Windows 语音 id，留空表示第一个 | `""` |
| 我开口时停止朗读 | `audio.stopTtsOnSpeech` | `true` |
| 口头禅词表 | `practice.fillersZh`、`practice.fillersEn` | 见[练习模式](/zh/guide/practice#review-report) |

## 诊断

| 设置 | 键名 | 默认值 |
| --- | --- | --- |
| 调试日志 | `diagnostics.debugLogging` | `false` |
