---
name: passage
title: 读一段
version: 1
description: 划选的一段或几段：按内容出概念、论证、术语解释
entries: [selection]
produces: [concept, argument, note]
steps:
  - id: read
    kind: model
    title: 读这几段
    produces: [concept, argument, note]
    input: [scope, translation]
    max_output_tokens: 8000
---

# 读一段

## 总则
读的是读者划选的几段原著正文和对齐的中译。按内容决定出什么：有定义就出概念卡，有推理就出论证卡，有难懂的词、典故、历史背景就出笔记（术语解释）。
- 只依据给出的段落。
- 原文句保持英文原样，解释用中文。

## 步骤 read
- 概念卡：标题「英文词 / 中文词」，原文 + 解释；
- 论证卡：标题一句话概括结论，前提和结论用原文句，推理用解释连起来；
- 笔记：一个术语或典故一张，标题写这个词，用解释说清楚，能挂段落号就挂。
一般出 2–5 张，宁少勿滥。
