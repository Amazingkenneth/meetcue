# 识别与提示质量

本页用于没有出现提示、不该出现时出现提示，或识别出错字的情况。

## 对方提问了但没有出现提示 {#no-hints}

**可能原因**

1. 拾取已暂停，或系统声音没有电平。
2. 开启了 **只提示指向我的问题**，而这个问题看起来是问别人的。
3. 问题很短或只是客套反问（“对吧？”“是吗？”），这类句子会被有意忽略。
4. 要点生成失败或超时（[MC-LLM-001](/zh/troubleshooting/error-codes#mc-llm-001)、[MC-LLM-005](/zh/troubleshooting/error-codes#mc-llm-005)）。
5. 达到了预算上限，且选择了 **暂停要点生成**（[MC-BUD-002](/zh/troubleshooting/error-codes#mc-bud-002)）。

**检查**

1. 查看面板标题栏的状态点：绿色表示正在拾取。
2. 打开 **设置 › 诊断**。最近事件会显示这句话是否被识别，以及检测的判断结果。

**解决**

1. 按 `Ctrl+Alt+Space` 开始拾取；电平不动请参考[音频与设备](/zh/troubleshooting/audio)。
2. 如果希望每个问题都有提示，关闭 **只提示指向我的问题**。
3. 出现错误码时，按错误提示中的办法处理。

**仍未解决？** [导出诊断信息](/zh/troubleshooting/diagnostics)并[提交 Issue](https://github.com/Amazingkenneth/meetcue/issues/new?template=bug.yml)。

## 闲聊也出现提示 {#too-many}

**可能原因**

1. 检测模型对你的会议来说太弱。
2. 多人之间互相提问，并不是问你的。

**检查**

1. 点开对话中的灰色问题原文，查看识别依据。

**解决**

1. 打开 **设置 › 提示 › 只提示指向我的问题**。
2. 填写 **我的角色**，让检测知道哪些问题该由你回答。
3. 在 **设置 › 供应商与费用** 中换一个更强的检测模型。

**仍未解决？** [导出诊断信息](/zh/troubleshooting/diagnostics)并[提交 Issue](https://github.com/Amazingkenneth/meetcue/issues/new?template=bug.yml)。

## 人名和术语识别错误 {#recognition}

**可能原因**

1. 会议语言设置不对。
2. 识别模型不认识产品名和缩写。
3. 音量太小或噪音大。

**检查**

1. 打开 **设置 › 音频 › 会议语言**。
2. 有人说话时观察对话中的转写。

**解决**

1. 选择实际使用的语言。同一句话里中英混说时选 **中文和英文**。
2. 把议程或项目资料添加为[会前资料](/zh/guide/materials)，其中的关键术语会作为热词传给识别服务。
3. 换另一家识别供应商试试：Soniox 对一句话里中英混说处理得很好。

**仍未解决？** [导出诊断信息](/zh/troubleshooting/diagnostics)并[提交 Issue](https://github.com/Amazingkenneth/meetcue/issues/new?template=bug.yml)。

## 一个问题被切成两段，或提示只回答了一半 {#split}

说话人在问题中间停顿时，MeetCue 会把后半句接回同一个问题：5 秒内开始、以连接词开头（“还有……”“以及……”“and…”）或补全上一句悬空结尾（“……和”“……，”）的句子会并入同一个问题，提示会重写以覆盖每个部分。问题之后紧跟的其他短句会交给检测模型判断，也可能被并入。

**可能原因**

1. 说话人停顿超过 5 秒，或接下去的话听起来不像是接着问。
2. 语音识别听错了连接词。

**检查**

1. 查看是否有两个相邻的问题块，第一个在半句话处结束。

**解决**

1. 调大 **设置 › 音频 › 高级 › 静音多久算一句结束**，例如从 700 毫秒调到 1000 毫秒。提示会稍晚一点出现。

**仍未解决？** [导出诊断信息](/zh/troubleshooting/diagnostics)并[提交 Issue](https://github.com/Amazingkenneth/meetcue/issues/new?template=bug.yml)。
