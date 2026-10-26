# 会前资料

本页介绍如何把你自己和这场会议的背景告诉 MeetCue，让提示更贴近你真正会说的话。

## 描述你的角色

在 **设置 › 提示 › 我的角色** 中用一句话介绍自己，例如：

> 网络架构师，8 年数据中心运维经验

要点模型会从这个角度组织要点。

## 添加文件

1. 打开 **设置 › 提示**。
2. 在 **会前资料** 下点击 **添加文件…**。
3. 选择议程、简历、项目资料等文件。支持 `.txt`、`.md`、`.docx` 和 `.pdf`。

MeetCue 在本机抽取文字并显示字数。没有文字层的扫描版 PDF 抽不出文字，请先用文字识别功能导出。

## 会发送什么

- 资料的一小段摘录会放进要点模型的指令中，让提示能提到你的项目和议程。
- 资料中最有辨识度的词（产品名、缩写、人名）会作为热词发给语音识别供应商，让这些词识别得更准。
- 全文只留在你的电脑上，也不会包含在诊断信息里。

资料放在指令中整场会议都不变的部分。支持提示缓存的供应商从第二个问题起对这部分收费会低很多。

想配合模拟面试试一试，可以用示例职位描述：[jd-software-engineer.md](https://github.com/Amazingkenneth/meetcue/blob/HEAD/samples/materials/jd-software-engineer.md) 或 [jd-security-ai.md](https://github.com/Amazingkenneth/meetcue/blob/HEAD/samples/materials/jd-security-ai.md)。它们与[模拟面试](/zh/guide/mock-interview)中的内置示例面试对应。

## 保持精简

资料越长，每次请求越大、开始越慢。与会议相关的一两页资料，比整份文档库效果更好。不再需要的文件可以点 **移除**。
