---
name: ask
title: 问这本书
version: 1
description: 先在本地找相关段落，读者勾选后再回答
entries: [question]
produces: [note, concept, argument]
steps:
  - id: answer
    kind: model
    title: 回答
    produces: [note, concept, argument]
    input: [question, scope, translation]
    max_output_tokens: 8000
---

# 问这本书

## 总则
读者问了一个关于这本书的问题，并从本地检索的结果里勾选了相关段落。只依据这些段落回答；段落里没有答案就照实说。

## 步骤 answer
- 先出一张笔记，标题就是问题，用三到六句回答：每个论点用原文句摆出依据，再用解释说明；
- 回答里出现了值得单独记的概念或论证，可以另出概念卡、论证卡；
- 段落不足以回答的部分，用解释写明「给出的段落里没有说」，不要猜。
