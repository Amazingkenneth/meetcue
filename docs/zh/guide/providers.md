# 供应商配置

本页逐一介绍支持的供应商：在哪里获取密钥、填在哪里、如何验证，以及最常见的错误。

所有密钥只保存在 Windows 凭据管理器中名为 `MeetCue` 的条目里，不会写入配置文件或日志；界面只知道某个密钥“已配置”还是“未配置”。

价格见[价格参考](/zh/reference/pricing)，以供应商官网为准。

## Soniox

语音识别，能很好地处理一句话里中英混说，自带句末检测。按音频小时计费。

1. **获取密钥**：登录 Soniox 控制台，创建一个 API 密钥。
2. **填写**：**设置 › 供应商与费用 › API 密钥 › Soniox 密钥**，然后点 **保存并验证**。
3. **验证**：验证成功后会列出你的账号可用的实时识别模型。
4. **端点**：Soniox 提供多个端点。保持 **自动**，运行[延迟探测](/zh/guide/latency)选出最快的一个。

常见错误：[MC-ASR-001](/zh/troubleshooting/error-codes#mc-asr-001)（密钥被拒绝）、[MC-ASR-002](/zh/troubleshooting/error-codes#mc-asr-002)（无法连接）。

## Deepgram

流式语音识别，按音频分钟计费。

1. **获取密钥**：登录 Deepgram 控制台，打开你的项目，创建一个具有成员权限的 API 密钥。
2. **填写**：**设置 › 供应商与费用 › API 密钥 › Deepgram 密钥**，然后点 **保存并验证**。
3. **验证**：验证成功后会列出你的项目和可用的识别模型。
4. **语言**：在 **设置 › 音频 › 会议语言** 中设置。选中中文时 Deepgram 使用普通话模型，否则使用多语言模型。

常见错误：[MC-ASR-001](/zh/troubleshooting/error-codes#mc-asr-001)、[MC-ASR-003](/zh/troubleshooting/error-codes#mc-asr-003)（请求被限流）。

## Anthropic

用 Claude 模型做问题检测和要点生成。

1. **获取密钥**：登录 Claude Console，打开 **API keys** 并创建密钥。为组织充值或添加付款方式。
2. **填写**：**设置 › 供应商与费用 › API 密钥 › Anthropic 密钥**，然后点 **保存并验证**。
3. **验证**：验证时从接口加载模型列表。价格表里有的模型显示价格，其余显示 **价格未知**。

常见错误：[MC-LLM-002](/zh/troubleshooting/error-codes#mc-llm-002)（密钥被拒绝）、[MC-LLM-003](/zh/troubleshooting/error-codes#mc-llm-003)（请求被限流）、[MC-LLM-004](/zh/troubleshooting/error-codes#mc-llm-004)（模型不可用）。

## Amazon Bedrock

通过你的 AWS 账号使用 Claude 模型。

1. **开通模型**：在 Amazon Bedrock 控制台中，切换到你要使用的 AWS 区域，为计划使用的 Anthropic 模型申请访问权限。
2. **获取密钥**：在 Bedrock 控制台创建 Bedrock API 密钥。MeetCue 以 Bearer 令牌的方式发送它。
3. **填写**：在 **设置 › 供应商与费用** 中填写开通模型的 **AWS 区域**，再在 **Bedrock 密钥** 处粘贴密钥并点 **保存并验证**。
4. **验证**：这类密钥无法获取 Bedrock 的模型列表，MeetCue 会显示价格表中的 Claude 模型。如果账号里没有开通某个模型，使用时会报 [MC-LLM-004](/zh/troubleshooting/error-codes#mc-llm-004)。

## OpenAI 兼容接口 {#openai-compatible-apis}

任何实现了 OpenAI Chat Completions 流式接口的服务，包括自建的模型服务。

1. **Base URL**：在 **设置 › 供应商与费用** 中填写以 `/v1` 结尾的地址，例如 `https://llm.example.com/v1`。
2. **密钥**：在 **OpenAI-compatible 密钥** 处粘贴该服务的 API 密钥。不需要密钥的服务可以留空。
3. **验证**：**保存并验证** 会加载 `/v1/models`。这些模型没有内置价格，请用 **填写价格** 补上，费用估算和预算才能生效。

部分服务在流式输出时不返回 token 用量，此时其用量和费用显示为零。

## 公司网络 {#corporate-networks}

MeetCue 使用 Windows 证书库，公司安装的证书会被信任。HTTPS 默认跟随系统代理设置；要使用其他代理，在 **设置 › 供应商与费用 › 高级 › HTTPS 代理** 中填写。如果连接失败并显示 [MC-NET-001](/zh/troubleshooting/error-codes#mc-net-001)，请看[延迟偏高](/zh/troubleshooting/latency#company-network)。
