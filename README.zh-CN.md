# MeetCue

[English](README.md) · **简体中文**

MeetCue 是一款桌面会议提词与面试练习工具。开会时，它同时拾取与会者的声音（系统声音）和你的声音（麦克风），识别出对方向你提出的问题，并在一个始终置顶、不抢焦点的面板里给出编号要点。你回答时，讲到的要点会自动打勾，下一条高亮显示。

练习模式可以按真实会议的节奏回放以往会议的字幕或面试问题列表，结束后生成复盘报告。

MeetCue 完全在你的电脑上运行，直接调用你选择的语音识别和大模型接口，使用你自己的密钥，没有 MeetCue 服务器。

- **文档：** <https://amazingkenneth.github.io/meetcue/zh/>
- **安装（Windows）：** `scoop bucket add meetcue https://github.com/Amazingkenneth/meetcue`，然后 `scoop install meetcue`
- **无需密钥试用：** 运行 `MeetCue.exe --demo` 回放一段示例面试。

## 功能

- `Ctrl+Alt+Space` 开始或暂停拾取；面板不抢焦点，共享屏幕时可以隐藏。
- 问题结束后约 1.5 秒出现编号要点，点击即可查看完整示例回答。
- 问题队列同时显示上一问、当前和下一问；插话问题自动排队。
- 内置延迟探测，实测各供应商在你的网络下的速度，并与价格并排展示，由你自己选择组合。
- 按调用统计费用、显示每月用量，可设置预算。
- 界面支持英文和简体中文，可随时切换。

## 供应商

| 环节 | 供应商 |
| --- | --- |
| 语音识别 | Soniox、Deepgram |
| 大模型 | Anthropic（Claude）、任意 OpenAI 兼容接口 |

## 从源码构建

需要 Rust（stable）、Node.js 22，Linux 上还需要 WebKitGTK 开发包。

```sh
npm ci
npm run dev:mock          # 在浏览器中预览界面，无需密钥
cargo test --workspace --exclude meetcue-app
npm test
npx tauri dev             # 运行桌面应用（在 app/src-tauri 目录）
```

参见[开发环境](https://amazingkenneth.github.io/meetcue/zh/contribute/development)与[架构](https://amazingkenneth.github.io/meetcue/zh/contribute/architecture)。

## 许可证

[MIT](LICENSE)
