# 安装 MeetCue

本页介绍如何在 Windows 上安装、升级和卸载 MeetCue。全程不需要管理员权限。

## 系统要求

- Windows 10 2004 及以上版本，或 Windows 11，64 位。
- Microsoft Edge WebView2 运行时。几乎所有 Windows 10 和 11 电脑都已预装。如果 MeetCue 提示 [MC-SYS-001](/zh/troubleshooting/error-codes#mc-sys-001)，请从微软官网安装 Evergreen 版运行时。
- 一个语音识别供应商的 API 密钥，和一个大模型供应商的 API 密钥。参见[供应商配置](/zh/guide/providers)。

::: info macOS 与 Linux
MeetCue 可以在 macOS 和 Linux 上编译运行，但当前版本只在 Windows 上支持音频采集。
:::

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
