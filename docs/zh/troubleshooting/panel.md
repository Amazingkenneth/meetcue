# 快捷键与悬浮窗

本页用于快捷键无效，或面板抢焦点、消失、在共享屏幕中被看到的情况。

## 快捷键没反应 {#shortcuts}

**可能原因**

1. 其他程序先注册了同样的组合键（[MC-HK-001](/zh/troubleshooting/error-codes#mc-hk-001)）。
2. 前台窗口属于以管理员身份运行的程序。
3. 游戏或播放器处于独占全屏。
4. MeetCue 没有在运行。

**检查**

1. 在任务栏通知区域查找 MeetCue 图标。
2. 打开 **设置 › 快捷键**，查看是否有标红的行。
3. 点击桌面后再试一次快捷键。

**解决**

1. 把标红的快捷键改成未被占用的组合。
2. Windows 不会把管理员窗口里的按键传给普通程序。这类窗口在前台时请用面板按钮，或不要以管理员身份运行该程序。
3. 把全屏程序切换为窗口或无边框窗口模式。

**仍未解决？** [导出诊断信息](/zh/troubleshooting/diagnostics)并[提交 Issue](https://github.com/Amazingkenneth/meetcue/issues/new?template=bug.yml)。

## 点击面板后会议窗口失去焦点 {#focus}

**可能原因**

1. 面板里的输入框或菜单需要键盘焦点。
2. 远程桌面或虚拟机的窗口管理忽略了“不激活”样式。

**检查**

1. 点击一条要点，然后在会议聊天框里打字，文字应该进入聊天框。

**解决**

1. 会议中用快捷键代替鼠标。
2. 打开 **设置 › 通用 › 鼠标穿透面板**，面板将完全不响应鼠标。

**仍未解决？** [导出诊断信息](/zh/troubleshooting/diagnostics)并[提交 Issue](https://github.com/Amazingkenneth/meetcue/issues/new?template=bug.yml)。

## 面板跑到屏幕外或找不到 {#position}

**可能原因**

1. 面板原本在外接显示器上，而显示器已断开。
2. 面板被隐藏了；`Ctrl+Alt+H` 在完整 → 迷你 → 隐藏之间循环。

**检查**

1. 按一两次 `Ctrl+Alt+H`。

**解决**

1. 右键托盘图标，选择 **重置面板位置**。

**仍未解决？** [导出诊断信息](/zh/troubleshooting/diagnostics)并[提交 Issue](https://github.com/Amazingkenneth/meetcue/issues/new?template=bug.yml)。

## 共享屏幕时对方能看到面板 {#screen-share}

**可能原因**

1. **共享屏幕时隐藏面板** 被关闭了。
2. 录制方式直接采集显示输出。部分录屏软件、远程协助工具和外接采集设备会这样做。
3. Windows 10 2004 之前的版本不支持对截屏隐藏窗口。

**检查**

1. 打开 **设置 › 通用**，查看 **共享屏幕时隐藏面板**。
2. 向自己或同事共享屏幕，或用 `Win+Shift+S` 截图：截图中不应出现面板。

**解决**

1. 打开这项设置。
2. 只共享单个窗口而不是整个屏幕。面板不属于其他程序的窗口，不会被共享出去。
3. 无法确定隐藏是否生效时，改用迷你条，或用 `Ctrl+Alt+H` 隐藏面板。

**仍未解决？** [导出诊断信息](/zh/troubleshooting/diagnostics)并[提交 Issue](https://github.com/Amazingkenneth/meetcue/issues/new?template=bug.yml)。
