# 开发环境

本页介绍如何从源码构建 MeetCue、运行测试，以及在没有 Windows 或 API 密钥的情况下预览界面。

## 环境要求

- [Rust](https://rustup.rs) 稳定版，仓库中的 `rust-toolchain.toml` 会自动选择。
- Node.js 20 或更高版本。
- Windows：Microsoft C++ 生成工具和 WebView2 运行时。
- Linux（仅构建桌面应用时需要）：[Tauri 依赖的系统库](https://v2.tauri.app/start/prerequisites/)（WebKitGTK、libappindicator 等）。
- macOS：需要 macOS 13 及以上版本，以及 Xcode 命令行工具（`xcode-select --install`）。在 `app/src-tauri` 中运行 `npx tauri build` 生成安装包，使用应用包获得稳定的隐私权限标识。

克隆仓库并安装 JavaScript 依赖：

```sh
git clone https://github.com/Amazingkenneth/meetcue
cd meetcue
npm ci
```

## 运行测试

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --exclude meetcue-app
npm run lint
npm run typecheck
npm test
npm run check:locales
npm run check:versions
```

测试不会访问网络，也不调用真实供应商。端到端管线测试用 mock 供应商回放 `samples/transcripts/sdn-interview.jsonl`。

调用真实供应商的测试标记为 `#[ignore]`，从环境变量读取密钥。只在你自己的电脑上运行：

```sh
MEETCUE_ANTHROPIC_KEY=... cargo test -p meetcue-pipeline --test live_providers -- --ignored
```

## 在浏览器中预览界面

```sh
npm run dev:mock
```

打开 `http://localhost:1420`。模拟后端通过真实组件回放一段录好的示例面试，界面库可以在面板、设置、首次设置向导和练习视图之间切换。任何操作系统上都能运行。

## 运行桌面应用

```sh
npx tauri dev
```

在 `app/src-tauri` 目录下运行。构建出的程序支持以下参数：

| 参数 | 作用 |
| --- | --- |
| `--demo` | 用 mock 供应商回放示例面试 |
| `--self-test` | 检查音频、快捷键、截屏隐藏和供应商，输出报告后退出 |
| `--version` | 输出版本号 |

`MEETCUE_CONFIG_DIR` 可以让应用使用其他设置文件夹，`MEETCUE_LOG` 用于设置日志过滤规则。

## 编写文档

```sh
npm run docs:dev
npm run docs:build
npm run docs:lint
```

`docs:build` 还会生成错误码和价格页面，并检查每个英文页面都有对应的中文页面且二级标题数量一致。

## 生成的文件

设置、事件和命令返回值的 TypeScript 类型在运行 core 测试时由 ts-rs 根据 Rust 类型生成。修改界面用到的 Rust 类型后，运行 `cargo test -p meetcue-core` 并提交 `app/ui/src/bindings` 中变化的文件。如果它们没有更新，CI 会失败。
