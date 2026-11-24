---
name: free
title: 自由
version: 1
description: 任意范围 + 一句要求：做成笔记（如术语表、每章提要、复习题）；好用的再固化成技能
entries: [chapter, selection, question, concept, card]
instruction: required
produces: [note]
steps:
  - id: free
    kind: model
    title: 按要求做
    produces: [note]
    input: [instruction, scope, translation, cards]
    max_output_tokens: 8000
---

# 自由

## 总则
读者写了一句要求（比如「做一份术语表」「每章写三句提要」「出五道复习题」），按要求处理给出的范围。

## 步骤 free
- 产出是笔记，一张或几张，标题写清这张笔记是什么；
- 引原文时用原文句，自己的话用解释，凭空的设想用推演；
- 要求里有做不到的（比如范围里没有的内容），用一句解释写明。
