# 音频与设备

本页用于 MeetCue 听不到你、听不到其他与会者，或听错了声音的情况。按步骤排查时，可以打开 **设置 › 音频 › 测试电平** 同时观察两个电平条。

## 系统声音电平一直不动 {#no-system-sound}

**可能原因**

1. 会议软件使用的播放设备不是 Windows 的默认输出设备。
2. 当前没有声音在播放，或会议软件被静音。
3. 其他程序以独占模式占用了输出设备（[MC-AUD-001](/zh/troubleshooting/error-codes#mc-aud-001)）。
4. 声卡驱动不支持环回采集（[MC-AUD-005](/zh/troubleshooting/error-codes#mc-aud-005)）。

**检查**

1. 打开 Windows **设置 › 系统 › 声音**，记下输出设备。
2. 在会议软件的音频设置里查看它使用的扬声器。
3. 在浏览器中播放一段视频，观察系统声音电平条。

**解决**

1. 把会议软件的扬声器设为 **默认**，或与 Windows 输出设备一致。
2. 如果提示 MC-AUD-001：打开 **声音 › 更多声音设置**，打开该设备的 **属性 › 高级**，取消勾选 **允许应用程序独占控制该设备**，然后重启占用设备的程序。
3. 从电脑厂商处更新声卡驱动。

::: tip 用耳机测试
用外放测试时，麦克风会录到外放声音，麦克风电平条会跳动，但这并不说明系统声音采集正常。请把扬声器静音或戴上耳机，只看系统声音电平条。`MeetCue.exe --self-test` 检查的正是这一点。
:::

**仍未解决？** [导出诊断信息](/zh/troubleshooting/diagnostics)并[提交 Issue](https://github.com/Amazingkenneth/meetcue/issues/new?template=bug.yml)。

## 麦克风电平一直不动 {#no-microphone}

**可能原因**

1. Windows 禁止桌面应用访问麦克风（[MC-AUD-002](/zh/troubleshooting/error-codes#mc-aud-002)）。
2. MeetCue 使用的麦克风不是你说话用的那个。
3. 麦克风被硬件按键或驱动静音。

**检查**

1. 打开 Windows **设置 › 隐私和安全性 › 麦克风**。
2. 在 MeetCue 中打开 **设置 › 音频**，查看 **麦克风（我）**。

**解决**

1. 打开 **麦克风访问权限** 和 **允许桌面应用访问你的麦克风**。
2. 在 **麦克风（我）** 中选择你的麦克风，或选 **系统默认** 并在 Windows 中设置默认设备。
3. 如果笔记本的麦克风静音键指示灯亮着，按一下取消静音。

**仍未解决？** [导出诊断信息](/zh/troubleshooting/diagnostics)并[提交 Issue](https://github.com/Amazingkenneth/meetcue/issues/new?template=bug.yml)。

## 换了耳机或音箱后采集停止 {#device-changed}

**可能原因**

1. 默认设备发生变化，采集正在重建（[MC-AUD-004](/zh/troubleshooting/error-codes#mc-aud-004)）。
2. 新设备被其他程序独占。

**检查**

1. 留意面板上“音频设备已切换，采集已自动恢复”的提示。
2. 切换后观察电平条几秒钟。

**解决**

1. 等两三秒，MeetCue 会自动重建采集。
2. 如果电平条仍不动，按 `Ctrl+Alt+Space` 暂停再继续。

**仍未解决？** [导出诊断信息](/zh/troubleshooting/diagnostics)并[提交 Issue](https://github.com/Amazingkenneth/meetcue/issues/new?template=bug.yml)。

## 说话轻的参会者的问题被漏掉 {#quiet-voices}

**可能原因**

1. 这位参会者的声音传到你电脑时比其他人轻得多，低于系统声音人声门限，因此没有被送去识别。
2. **放大会议中轻声说话的人** 已关闭，识别服务收到的声音太轻，无法可靠转写。
3. 会议软件里给这位参会者或整体输出设置的音量较低。

**检查**

1. 打开 **设置 › 音频**，点 **测试电平**，在这个人说话时观察系统声音电平条，记下 dB 数值。
2. 与 **设置 › 音频 › 高级 › 系统声音人声门限** 对比。

**解决**

1. 打开 **放大会议中轻声说话的人**（默认开启）。它会在识别前把轻声提升到正常说话音量，不会改变你的麦克风。
2. 把 **系统声音人声门限** 调到比你看到的电平低约 5 dB。默认值 −60 dBFS 几乎能拾取所有人声；只有会议里的音乐或背景噪声被转写时才需要调高。
3. 在会议软件里调高这位参会者的音量。

**仍未解决？** [导出诊断信息](/zh/troubleshooting/diagnostics)并[提交 Issue](https://github.com/Amazingkenneth/meetcue/issues/new?template=bug.yml)。

## 对方的话把我的要点勾掉了 {#echo}

**可能原因**

1. 你在用外放，麦克风录到了其他与会者的声音。
2. 麦克风很灵敏，或离扬声器太近。

**检查**

1. 查看对话：标为 **我** 的句子在重复别人刚说的话。

**解决**

1. 戴耳机，这是最可靠的办法。
2. 调低扬声器音量，或让麦克风远离扬声器。
3. 把 **设置 › 音频 › 高级 › 麦克风人声门限** 调高一点。

**仍未解决？** [导出诊断信息](/zh/troubleshooting/diagnostics)并[提交 Issue](https://github.com/Amazingkenneth/meetcue/issues/new?template=bug.yml)。

## 使用蓝牙耳机时播放音质变差 {#bluetooth}

**可能原因**

1. 程序打开蓝牙耳机的麦克风时，Windows 会把耳机切换到通话模式，所有声音的音质都会下降。

**检查**

1. 所选麦克风看起来是蓝牙设备时，**设置 › 音频** 会显示提醒。

**解决**

1. 在 **麦克风（我）** 中选择电脑内置麦克风，耳机只用来听。

**仍未解决？** [导出诊断信息](/zh/troubleshooting/diagnostics)并[提交 Issue](https://github.com/Amazingkenneth/meetcue/issues/new?template=bug.yml)。
