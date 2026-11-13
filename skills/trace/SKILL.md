---
name: trace
title: 追踪概念
version: 1
description: 一个概念的中英两种写法在全书里检索，按章列出用法与变化
entries: [concept]
produces: [concept, note]
steps:
  - id: trace
    kind: model
    title: 按章列出用法
    produces: [concept, note]
    input: [terms, scope, translation]
    max_output_tokens: 10000
---

# 追踪概念

## 总则
读者要追踪一个概念（给了英文和中文的检索词），并勾选了全书里命中的段落，按章排好。目标是看清这个概念在各章怎么用、意思有没有变化。

## 步骤 trace
- 先出一张概念卡：标题「英文词 / 中文词」，用原文句摆出最核心的一处用法，再用解释总结它在全书里的意思；
- 每一章出一张笔记：标题写「章的段落号前缀：这一章怎么用」（如「I.i：分工的效果」），用原文句摆出这一章的用法，用解释说明和前面各章比有什么变化；
- 只依据给出的段落；没勾选的章不写。
