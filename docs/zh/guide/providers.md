# 供应商配置

本页逐一介绍支持的供应商：在哪里获取密钥、填在哪里、如何验证，以及最常见的错误。

所有密钥只保存在 Windows 凭据管理器中名为 `MeetCue` 的条目里，不会写入配置文件或日志；界面只知道某个密钥“已配置”还是“未配置”。

在 **设置 › 供应商与费用 › 供应商与模型** 中为每项任务选择供应商并填写模型名。MeetCue 不列出模型和价格：保存密钥后，供应商返回的模型名会作为输入建议，每个供应商旁都有 **查看官网价格** 链接指向其官方价格页。

## Soniox

语音识别，能很好地处理一句话里中英混说，自带句末检测。按音频小时计费。

1. **获取密钥**：登录 Soniox 控制台，创建一个 API 密钥。
2. **填写**：**设置 › 供应商与费用 › 接口地址与密钥 › Soniox 密钥**，然后点 **保存并验证**。
3. **验证**：验证成功后会列出你的账号可用的实时识别模型。
4. **端点**：Soniox 提供多个端点。保持 **自动**，运行[延迟探测](/zh/guide/latency)选出最快的一个。

常见错误：[MC-ASR-001](/zh/troubleshooting/error-codes#mc-asr-001)（密钥被拒绝）、[MC-ASR-002](/zh/troubleshooting/error-codes#mc-asr-002)（无法连接）。

## Deepgram

流式语音识别，按音频分钟计费。

1. **获取密钥**：登录 Deepgram 控制台，打开你的项目，创建一个具有成员权限的 API 密钥。
2. **填写**：**设置 › 供应商与费用 › 接口地址与密钥 › Deepgram 密钥**，然后点 **保存并验证**。
3. **验证**：验证成功后会列出你的项目和可用的识别模型。
4. **语言**：在 **设置 › 音频 › 会议语言** 中设置，主要语言放在前面。Deepgram 无法在同一路音频中同时识别中文和英文，两者都选时只识别主要语言；中英混说的会议更适合用 Soniox。

常见错误：[MC-ASR-001](/zh/troubleshooting/error-codes#mc-asr-001)、[MC-ASR-003](/zh/troubleshooting/error-codes#mc-asr-003)（请求被限流）。

## 阿里云（Fun-ASR） {#alibaba-cloud}

阿里云百炼（Model Studio）的实时语音识别，针对普通话设计，能识别夹杂的英文术语和多种方言。按音频秒数计费。

1. **获取密钥**：在百炼控制台顶部选择地域（中国大陆以外选新加坡，大陆以内选北京），再在该地域创建 API Key。密钥只能在创建它的地域使用。
2. **填写**：**设置 › 供应商与费用 › 接口地址与密钥 › Alibaba Cloud (Fun-ASR) 密钥**，然后点 **保存并验证**。
3. **验证**：验证会开始并结束一个空的识别任务，不消耗音频。
4. **接口地址**：MeetCue 内置两个地域的地址。如果所选地址拒绝密钥，会自动尝试另一个地域，并继续使用可用的那个。
5. **模型**：`fun-asr-realtime`（默认）会把热词作为上下文使用。`paraformer-realtime-v2` 只在北京地域提供。

常见错误：[MC-ASR-001](/zh/troubleshooting/error-codes#mc-asr-001)（两个地域都拒绝密钥）、[MC-ASR-003](/zh/troubleshooting/error-codes#mc-asr-003)（请求被限流或账户欠费）。

## 腾讯云语音识别 {#tencent-cloud}

腾讯云的实时语音识别，支持普通话、英语、粤语和多种方言。按音频时长计费。

1. **获取密钥**：在腾讯云控制台开通语音识别，然后打开 **访问管理 › API 密钥管理**。需要 **AppID**（在同一页面和账号信息中可以看到）、**SecretId** 和对应的 **SecretKey**。
2. **填写**：在 **设置 › 供应商与费用 › 接口地址与密钥 › Tencent Cloud ASR 密钥** 中，按 `AppID:SecretId:SecretKey` 的格式一次粘贴三项，然后点 **保存并验证**。
3. **验证**：验证会建立一个签名连接，不发送音频就关闭。
4. **模型**：`16k_zh_en`（默认）能识别同一句中的中英文以及 31 种方言。`16k_zh` 是更便宜的普通话标准版，`16k_yue` 为粤语，`16k_en` 为英语。
5. **在中国大陆以外使用**：中国站账号在境外可能被拒绝。MeetCue 会以密钥错误的形式显示腾讯云的提示，此时请改用国际站账号。

常见错误：[MC-ASR-001](/zh/troubleshooting/error-codes#mc-asr-001)（签名被拒绝、未开通服务或境外访问）、[MC-ASR-003](/zh/troubleshooting/error-codes#mc-asr-003)（资源包用完、欠费或并发超限）。

## Anthropic

用 Claude 模型做问题检测和要点生成。

1. **获取密钥**：登录 Claude Console，打开 **API keys** 并创建密钥。为组织充值或添加付款方式。
2. **填写**：**设置 › 供应商与费用 › 接口地址与密钥 › Anthropic 密钥**，然后点 **保存并验证**。
3. **验证**：验证时从接口加载模型列表，其中的模型名会作为 **模型** 输入框的建议。也可以填写其他任意模型名。
4. **接口地址**（可选）：留空则使用 `https://api.anthropic.com`。如需经由兼容 Messages API 的网关或代理，填写其地址，例如 `https://gateway.example.com`（缺少 `/v1` 时 MeetCue 会自动补上）。

常见错误：[MC-LLM-002](/zh/troubleshooting/error-codes#mc-llm-002)（密钥被拒绝）、[MC-LLM-003](/zh/troubleshooting/error-codes#mc-llm-003)（请求被限流）、[MC-LLM-004](/zh/troubleshooting/error-codes#mc-llm-004)（模型不可用）。

## OpenAI 兼容接口 {#openai-compatible-apis}

任何实现了 OpenAI Chat Completions 流式接口的服务，包括自建的模型服务。

1. **接口地址**：为任务选择 **OpenAI-compatible**，然后在 **设置 › 供应商与费用 › 接口地址与密钥** 中填写以 `/v1` 结尾的地址，例如 `https://llm.example.com/v1`。
2. **密钥**：在 **OpenAI-compatible 密钥** 处粘贴该服务的 API 密钥。不需要密钥的服务可以留空。
3. **验证**：**保存并验证** 会加载 `/v1/models`，其中的模型名会作为 **模型** 输入框的建议。请填写你所用服务的模型名。
4. **价格**：如需费用估算和预算，请在 **设置 › 供应商与费用 › 预算 › 你填写的价格** 中填写该服务的价格。

部分服务在流式输出时不返回 token 用量，此时其用量和费用显示为零。

## 公司网络 {#corporate-networks}

MeetCue 使用 Windows 证书库，公司安装的证书会被信任。所有连接（包括语音识别）都跟随 Windows 设置中手动配置的代理（**网络和 Internet › 代理**）。MeetCue 不会执行自动配置脚本；如果你的网络使用自动配置脚本，或要使用其他代理，请在 **设置 › 供应商与费用 › 高级 › HTTPS 代理** 中填写。如果连接失败并显示 [MC-NET-001](/zh/troubleshooting/error-codes#mc-net-001)，请看[延迟偏高](/zh/troubleshooting/latency#company-network)。
