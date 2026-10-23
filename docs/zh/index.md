---
layout: home

hero:
  name: MeetCue
  text: 知道接下来该说什么
  tagline: 会议与面试的实时提词助手，并提供模拟练习帮你适应节奏。在你的电脑上运行，使用你自己的语音识别与大模型 API 密钥。
  image:
    src: /logo.png
    alt: MeetCue
  actions:
    - theme: brand
      text: 安装
      link: /zh/install
    - theme: alt
      text: 五分钟上手
      link: /zh/quick-start

features:
  - title: 会议提词
    details: 按 Ctrl+Alt+Space 开始。对方向你提问后约 1.5 秒，编号要点就会出现，你讲到哪条就自动打勾。
  - title: 模拟练习
    details: 导入以往会议的字幕或面试问题列表，MeetCue 按真实会议的节奏朗读问题，结束后给出复盘报告。
  - title: 延迟探测
    details: 实测各语音识别与大模型供应商在你的网络下的响应速度，对照价格，选出适合你的组合。
---

## 一分钟安装

Windows 版通过 [Scoop](https://scoop.sh) 安装，全程不需要管理员权限：

```powershell
scoop bucket add meetcue https://github.com/Amazingkenneth/meetcue
scoop install meetcue
```

然后按照[首次设置](/zh/setup)完成配置。

## 界面一览

示例面试（`MeetCue.exe --demo`）中的面板：第一个问题已答完，要点全部打勾；第二个问题正在回答，下一条要讲的要点高亮显示。

<img src="/screens/home-1-dark.zh.png" alt="MeetCue 面板，显示两个问题及其编号要点" width="440">
