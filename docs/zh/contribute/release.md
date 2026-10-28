# 发布流程

发布是自动的。每次推送到默认分支且改动了程序本身，CI 通过后就会发布新版本；不需要手动修改版本号或推送标签。

## 合并之前

1. 在两种语言[更新日志](/zh/reference/changelog)的 **未发布** 部分写上本次改动。发布时这些条目会移到新版本标题下，并用作发布说明。
2. 对照各供应商的定价页面核对 `crates/core/data/pricing.json` 中的价格，有变化时更新 `verified` 日期和 `updated`。程序从文档站下载这个文件，所以修正价格只需推送到默认分支，无需发布新版本。
3. 涉及音频、语音识别或窗口的改动，请在 Windows 和 macOS 电脑上用该分支的构建运行[实机测试清单](/zh/test-checklist)（CI 会为每次推送上传 `MeetCue-windows-x64`）。

## 版本号如何确定

新版本号是最新的 `v*` 标签把修订号加一。在拉取请求标题中，或在直接推送到默认分支的提交标题中加入标记，可以改变这一点。只出现在说明正文中的标记会被忽略，所以介绍这些标记不会误触发。

| 标记 | 效果 |
| --- | --- |
| `[release minor]` | 0.1.5 → 0.2.0 |
| `[release major]` | 0.1.5 → 1.0.0 |
| `[skip release]` | 写在最新的提交中：本次推送不发布 |

只改动文档、工作流或 Scoop manifest 的推送不会发布。需要手动发布时，在 Actions 页面运行 **Release** 工作流并选择要增加的部分。

## 发布工作流做了什么

默认分支上的 CI 完成后会运行 `.github/workflows/release.yml`：

1. 确定版本号。只发布最新的提交：如果 CI 运行期间分支又有新提交，由新提交的运行一并发布。
2. 在 Windows 上用这个版本号构建 `MeetCue.exe`，检查 `MeetCue.exe --version`，并把 `MeetCue.exe`、`LICENSE` 和 README 文件打包为 `meetcue-win-x64.zip`。
3. 运行 `scripts/set-version.mjs`：把版本号写入 `Cargo.toml`、`Cargo.lock`、各 `package.json` 和 `package-lock.json`，把更新日志中 **未发布** 的条目移到新版本下，并让 `bucket/meetcue.json` 指向新的 zip 及其 SHA-256 哈希。
4. 以仓库变量 `GIT_NAME` 和 `GIT_EMAIL` 的身份把这些改动提交到默认分支（"Release x.y.z"），并打上 `vx.y.z` 标签。提交和标签一起推送；如果分支在此期间有了新提交，则什么都不发布，由下一次运行发布。
5. 创建 GitHub Release，上传 zip，并以更新日志中的这一节作为发布说明，然后重新发布文档站。
6. 在 Windows 构建机上安装 Scoop，用标签中的 manifest 安装 MeetCue，确认输出新版本号。

`npm run check:versions` 会检查所有 manifest、两个锁文件和 Scoop manifest 的版本号一致，因此提交中的版本号始终等于最新发布的版本。哈希只由工作流在上传 zip 的同一次运行中计算并提交。不要手动替换 Release 中的文件；需要修改时请发布新版本。

如果提交和标签已推送后某个步骤失败，使用 **Re-run failed jobs**，它会为同一标签完成发布。

## 发布之后

1. 在 Windows 和 macOS 电脑上确认 `scoop update meetcue` 能升级到新版本。
2. 确认文档站点显示了新的更新日志。

## 仓库设置

以下设置只需配置一次：

- **Settings › Pages**：来源设为 `gh-pages` 分支。默认分支上 `docs/` 或 `errors.json` 有变化时，文档工作流会发布到这里。
- **Settings › Secrets and variables › Actions › Variables**：设置 `GIT_NAME` 和 `GIT_EMAIL`，供工作流提交时使用。
- **Settings › Actions › General › Workflow permissions**：工作流用 `GITHUB_TOKEN` 推送到默认分支，如果默认分支有保护规则，需要允许 GitHub Actions 推送。
- 仓库必须是公开的，否则 Scoop 无法下载 Release 中的文件。

Mac 发布在 Apple 芯片和 Intel 运行器上构建，提供 `.app` ZIP 和 DMG。应用包检查会验证隐私用途描述、最低 macOS 13 版本、签名完整性和 `--version`。当前采用临时签名；Developer ID 签名和公证需要 Apple 凭据，本次变更未配置。
