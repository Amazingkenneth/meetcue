# 安装与升级

本页用于 Scoop 无法安装或升级 MeetCue、Windows 拦截 MeetCue，或升级后设置似乎丢失的情况。

## Scoop 无法下载 MeetCue {#download}

**可能原因**

1. 无法访问 `github.com`，或需要代理。
2. 添加 bucket 时地址填错了。

**检查**

1. 运行 `scoop bucket list`，查看 `meetcue` bucket 的地址。
2. 在浏览器中打开 `https://github.com/Amazingkenneth/meetcue/releases`。

**解决**

1. 重新添加 bucket：

   ```powershell
   scoop bucket rm meetcue
   scoop bucket add meetcue https://github.com/Amazingkenneth/meetcue
   ```

2. 需要代理时，用 `scoop config proxy proxy.example.com:8080` 为 Scoop 设置代理。

**仍未解决？** [提交 Issue](https://github.com/Amazingkenneth/meetcue/issues/new?template=bug.yml)，附上 Scoop 的完整输出。

## Scoop 提示哈希不匹配 {#hash}

**可能原因**

1. 下载中断，或被代理修改。
2. 本地 bucket 版本过旧。

**检查**

1. 运行 `scoop update` 刷新所有 bucket。

**解决**

1. 清除缓存的下载后重试：

   ```powershell
   scoop cache rm meetcue
   scoop install meetcue
   ```

**仍未解决？** [提交 Issue](https://github.com/Amazingkenneth/meetcue/issues/new?template=bug.yml)，附上 Scoop 的完整输出。

## Windows 或杀毒软件拦截 MeetCue {#blocked}

**可能原因**

1. 本版本没有代码签名，SmartScreen 可能提示“未知发布者”。
2. 杀毒软件把新出现、下载量少的程序标记为可疑。
3. 公司策略只允许运行经批准的程序。

**检查**

1. 记下具体提示内容以及是哪个程序弹出的。

**解决**

1. 遇到 SmartScreen 时，如果你信任这个下载，点击 **更多信息 › 仍要运行**。Release 页面列出了 zip 的 SHA-256 哈希，可以核对。
2. 向杀毒软件厂商报告误报，或请 IT 部门放行 `MeetCue.exe`。
3. 如果公司策略拦截所有未签名程序，在提供签名版本之前 MeetCue 无法在该电脑上运行。

**仍未解决？** [提交 Issue](https://github.com/Amazingkenneth/meetcue/issues/new?template=bug.yml)。

## 提示缺少 WebView2 {#webview2}

**可能原因**

1. Microsoft Edge WebView2 运行时被卸载或从未安装（[MC-SYS-001](/zh/troubleshooting/error-codes#mc-sys-001)）。

**检查**

1. 在 **设置 › 应用 › 已安装的应用** 中查找 **Microsoft Edge WebView2 Runtime**。

**解决**

1. 从微软官网下载并安装 Evergreen 版 WebView2 运行时，然后重新启动 MeetCue。

**仍未解决？** [提交 Issue](https://github.com/Amazingkenneth/meetcue/issues/new?template=bug.yml)。

## 升级后设置不见了 {#settings-lost}

**可能原因**

1. MeetCue 无法写入程序所在文件夹，把设置保存到了 `%APPDATA%\MeetCue`（[MC-SYS-002](/zh/troubleshooting/error-codes#mc-sys-002)）。
2. 卸载时使用了 `--purge`。

**检查**

1. 打开 **设置 › 诊断 › 关于**，查看 **配置目录**。通过 Scoop 安装时应位于 `scoop\persist\meetcue\config` 下。

**解决**

1. 如果设置在 `%APPDATA%\MeetCue` 中，把 `settings.json` 和 `latency-history.json` 复制到 persist 文件夹，然后重启 MeetCue。
2. API 密钥保存在 Windows 凭据管理器中，升级和卸载都不会丢失。

**仍未解决？** [导出诊断信息](/zh/troubleshooting/diagnostics)并[提交 Issue](https://github.com/Amazingkenneth/meetcue/issues/new?template=bug.yml)。
