# 排错总览

从你看到的现象出发。每一条都链接到说明原因、检查和解决步骤的页面。

## 错误提示

MeetCue 的每个错误都带有形如 `MC-AUD-001` 的错误码。点击提示里的 **查看解决办法**，或在[错误码参考](/zh/troubleshooting/error-codes)中查找。

## 你看到的现象

| 现象 | 前往 |
| --- | --- |
| 系统声音电平一直不动 | [音频：没有系统声音](/zh/troubleshooting/audio#no-system-sound) |
| 麦克风电平一直不动 | [音频：没有麦克风声音](/zh/troubleshooting/audio#no-microphone) |
| 插上耳机后采集停止 | [音频：设备变化](/zh/troubleshooting/audio#device-changed) |
| 对方的话把我的要点勾掉了 | [音频：回声](/zh/troubleshooting/audio#echo) |
| 快捷键没反应 | [悬浮窗：快捷键](/zh/troubleshooting/panel#shortcuts) |
| 点面板会让会议窗口失去焦点 | [悬浮窗：焦点](/zh/troubleshooting/panel#focus) |
| 面板跑到屏幕外或找不到 | [悬浮窗：位置](/zh/troubleshooting/panel#position) |
| 共享屏幕时对方能看到面板 | [悬浮窗：共享屏幕](/zh/troubleshooting/panel#screen-share) |
| 对方提问了但没有出现提示 | [质量：没有提示](/zh/troubleshooting/quality#no-hints) |
| 闲聊也出现提示 | [质量：提示过多](/zh/troubleshooting/quality#too-many) |
| 人名和术语识别错误 | [质量：识别](/zh/troubleshooting/quality#recognition) |
| 提示要两三秒以上才出现 | [延迟偏高](/zh/troubleshooting/latency#slow-hints) |
| 在公司请求失败、在家正常 | [公司网络](/zh/troubleshooting/latency#company-network) |
| Scoop 无法安装或升级 MeetCue | [安装与升级](/zh/troubleshooting/install) |
| Windows 或杀毒软件拦截 MeetCue | [安装：被拦截](/zh/troubleshooting/install#blocked) |

## 寻求帮助

1. [导出诊断信息](/zh/troubleshooting/diagnostics)。文件中不包含 API 密钥、转写或会前资料。
2. [提交 Issue](https://github.com/Amazingkenneth/meetcue/issues/new?template=bug.yml)，写明错误码、MeetCue 版本和出现问题的步骤，并附上诊断文件。
