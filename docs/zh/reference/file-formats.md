# 文件格式

本页说明练习模式可以导入的文件及示例。文件可以是 UTF-8（带或不带 BOM）、UTF-16 或 GBK 等旧编码，MeetCue 会自动识别；Windows 和 Unix 换行符都支持。

## 字幕：SRT

标准 SubRip 文件。在文本开头写说话人名字，后接冒号。

```text
1
00:00:01,200 --> 00:00:04,800
面试官：先请你简单介绍一下自己？

2
00:00:05,500 --> 00:00:12,000
小林：好的。我做数据中心网络八年了。
```

毫秒前的分隔符用逗号或点号都可以。

## 字幕：WebVTT

说话人取自 `<v 名字>` 标签或“名字：”前缀。小时部分可以省略。

```text
WEBVTT

00:01.200 --> 00:04.800
<v 面试官>先请你简单介绍一下自己？

00:05.500 --> 00:12.000
<v 小林>好的。我做数据中心网络八年了。
```

## 字幕：纯文本

每行一句，可以带“名字：”前缀，也可以在方括号中带时间戳。全角冒号（`：`）和半角冒号都可以。

```text
[00:01] 面试官：先请你简单介绍一下自己？
[00:05] 小林：好的。我做数据中心网络八年了。
```

没有时间戳时，MeetCue 逐句依次播放。没有说话人时，由模型把每句标为提问或回答，你确认后再使用。

## 问题列表：文本与 Markdown

每行一题。以 `#` 开头的行开始一个分组，分组名会显示在预览和复盘中。空行会被忽略，Markdown 列表符号（`-`、`*`、`1.`）会被去掉。

```markdown
# 基础
- 控制平面和数据平面有什么区别？
- BGP 如何选择最优路径？

# 设计
- 如果要为两个数据中心设计网络，你会怎么做？
```

## 问题列表：CSV

第一行是列名。只有 `question` 是必填的。

| 列 | 含义 |
| --- | --- |
| `question` | 问题 |
| `reference_answer` | 参考回答，用于复盘反馈 |
| `category` | 分组名 |
| `followups` | 追问，用 `\|` 分隔 |

```text
question,reference_answer,category,followups
"什么是 SDN？","控制与转发分离；集中控制器；通过 API 编程。",基础,"你用过哪些控制器？|有哪些风险？"
```

## 示例文件

- [sdn-interview.srt](https://github.com/Amazingkenneth/meetcue/blob/HEAD/samples/practice/sdn-interview.srt)：一段中英混合的简短面试。
- [questions-network.csv](https://github.com/Amazingkenneth/meetcue/blob/HEAD/samples/practice/questions-network.csv)：十道网络架构面试题，附参考回答。
- [questions-software-engineer.csv](https://github.com/Amazingkenneth/meetcue/blob/HEAD/samples/practice/questions-software-engineer.csv)：十四道软件工程面试题，附参考回答和追问（英文）。
- [swe-interview.srt](https://github.com/Amazingkenneth/meetcue/blob/HEAD/samples/practice/swe-interview.srt)：一场后端软件工程师技术面（英文）。
- [questions-security-ai.csv](https://github.com/Amazingkenneth/meetcue/blob/HEAD/samples/practice/questions-security-ai.csv)：十二道网络安全与 AI 安全面试题，附参考回答和追问（英文）。
- [security-ai-interview.srt](https://github.com/Amazingkenneth/meetcue/blob/HEAD/samples/practice/security-ai-interview.srt)：一场 AI 平台团队的网络安全工程师面试（英文）。

这些文件都已内置在 MeetCue 中：在模拟面试窗口中点 **使用** 即可。见[模拟面试](/zh/guide/mock-interview)。
