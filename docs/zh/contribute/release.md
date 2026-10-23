# 发布流程

本页介绍 MeetCue 如何发布新版本：从修改版本号到 Scoop manifest 更新完成。

## 准备

1. 确认默认分支上的 CI 全部通过。
2. 对照各供应商的定价页面核对 `crates/core/data/pricing.json` 中的价格，并更新 `verified` 日期和 `updated`。
3. 在 Windows 电脑上用默认分支的构建运行[实机测试清单](/zh/test-checklist)。
4. 把两种语言[更新日志](/zh/reference/changelog)中 **未发布** 下的条目移到新版本标题下。

## 修改版本号

版本号只在根目录 `Cargo.toml` 的 `[workspace.package]` 中定义。同时把 `package.json`、`app/ui/package.json` 和 `docs/package.json` 改成相同的值；`npm run check:versions` 会检查它们是否一致。`tauri.conf.json` 从 Cargo 读取版本号。

## 打标签

```sh
git tag v0.2.0
git push origin v0.2.0
```

## 发布工作流做了什么

推送 `v*` 标签会在 Windows 上运行 `.github/workflows/release.yml`：

1. 用 Tauri CLI 构建 `MeetCue.exe`，不生成安装程序。
2. 把 `MeetCue.exe`、`LICENSE` 和 README 文件打包为 `meetcue-win-x64.zip`，并计算 SHA-256 哈希。
3. 创建 GitHub Release，上传 zip 并生成发布说明。
4. 用新的版本号、下载地址和哈希更新 `bucket/meetcue.json`，并以仓库变量 `GIT_NAME` 和 `GIT_EMAIL` 的身份提交到默认分支。
5. 在构建机上安装 Scoop，用更新后的 manifest 安装 MeetCue，确认 `MeetCue.exe --version` 输出新版本号。

哈希只由工作流在上传 zip 的同一次运行中计算并提交。不要手动替换 Release 中的文件；需要修改时请发布新版本。

## 发布之后

1. 在 Windows 电脑上确认 `scoop update meetcue` 能升级到新版本。
2. 确认文档站点显示了新的更新日志。

## 仓库设置

以下设置只需配置一次：

- **Settings › Pages**：来源设为 `gh-pages` 分支。默认分支上 `docs/` 或 `errors.json` 有变化时，文档工作流会发布到这里。
- **Settings › Secrets and variables › Actions › Variables**：设置 `GIT_NAME` 和 `GIT_EMAIL`，供工作流提交时使用。
- 仓库必须是公开的，否则 Scoop 无法下载 Release 中的文件。
