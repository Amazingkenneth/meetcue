# 安装 MeetCue

本页介绍如何在 Windows 和 macOS 上安装、升级和卸载 MeetCue。全程不需要管理员权限。

## 系统要求

- Windows 10 2004 及以上版本 / Windows 11（64 位），或 macOS 13 及以上版本（Apple 芯片或 Intel）。
- Windows 上需要 Microsoft Edge WebView2 运行时。几乎所有 Windows 10 和 11 电脑都已预装。如果 MeetCue 提示 [MC-SYS-001](/zh/troubleshooting/error-codes#mc-sys-001)，请从微软官网安装 Evergreen 版运行时。
- 一个语音识别供应商的 API 密钥，和一个大模型供应商的 API 密钥。参见[供应商配置](/zh/guide/providers)。

::: info Linux
MeetCue 可以在 Linux 上编译运行，但尚未实现原生音频采集后端。
:::

## 在 macOS 上安装

需要 macOS 13 Ventura 或更高版本，支持 Apple 芯片和 Intel。从 [Releases](https://github.com/Amazingkenneth/meetcue/releases) 下载 `meetcue-macos-arm64.dmg`（Apple 芯片）或 `meetcue-macos-x64.dmg`（Intel），打开后把 **MeetCue.app** 拖到 **应用程序**。发布前也可从拉取请求的 CI 下载应用 ZIP 测试。

当前 Mac 安装包采用临时签名，尚未经 Apple 公证。如果 macOS 拦截了从本仓库下载的应用，请在 **系统设置 › 隐私与安全性** 中针对 MeetCue 选择 **仍要打开**，不要全局关闭 Gatekeeper。

首次使用时，在 **系统设置 › 隐私与安全性** 中允许 **麦克风** 和 **屏幕与系统音频录制**（旧版系统为 **屏幕录制**）。授权后退出并重新打开 MeetCue。系统声音通过 ScreenCaptureKit 获取，不录制屏幕图像，也不需要虚拟声卡驱动。未在 设置 › 音频 中指定麦克风时，会跟随系统默认输入设备。

无需密钥即可体验：

```sh
/Applications/MeetCue.app/Contents/MacOS/MeetCue --demo
```

同一可执行文件加 `--self-test` 可检查音频和快捷键。请连接耳机，并在麦克风测试期间说话。API 密钥保存在 macOS 钥匙串中。更新时替换应用程序目录中的应用，会保留设置和密钥。要一并清除它们，先使用 **设置 › 通用 › 清除全部数据**，再删除应用。

## 用 Scoop 安装

1. 如果还没有安装 Scoop，打开 PowerShell 并运行：

   ```powershell
   Set-ExecutionPolicy -ExecutionPolicy RemoteSigned -Scope CurrentUser
   irm get.scoop.sh | iex
   ```

2. 添加 MeetCue 的 bucket：

   ```powershell
   scoop bucket add meetcue https://github.com/Amazingkenneth/meetcue
   ```

3. 安装 MeetCue：

   ```powershell
   scoop install meetcue
   ```

4. 从开始菜单启动 **MeetCue**。首次运行会打开设置向导，请继续阅读[首次设置](/zh/setup)。

## 升级

```powershell
scoop update meetcue
```

设置、延迟探测历史和练习记录保存在 Scoop 的 `persist` 目录中，API 密钥保存在 Windows 凭据管理器中，因此升级后都会保留。

## 卸载

```powershell
scoop uninstall meetcue
```

如果要同时删除设置，加上 `--purge`。API 密钥保留在 Windows 凭据管理器中名为 `MeetCue` 的条目里，如有需要请在那里删除；也可以在卸载前运行 **设置 › 通用 › 清除所有数据**，一并删除密钥和设置。

## 遇到问题

请参阅[安装与升级排错](/zh/troubleshooting/install)。
