---
name: chapter
title: 整章分析
version: 1
description: 一章的概念、论证和它们之间的关系；三步可以只选其中几步
entries: [chapter]
produces: [concept, argument, relation]
steps:
  - id: concepts
    kind: model
    title: 概念
    produces: [concept]
    input: [scope, translation]
    max_output_tokens: 16000
    optional: true
  - id: arguments
    kind: model
    title: 论证
    produces: [argument]
    input: [scope, translation, cards]
    max_output_tokens: 12000
    optional: true
  - id: relations
    kind: model
    title: 关系
    produces: [relation]
    input: [cards]
    max_output_tokens: 6000
    optional: true
---

# 整章分析

## 总则
读的是一章原著正文（英文，带段落号），旁边是对齐的中译。目标是帮读者读懂、复习和写作时引用。
- 只依据给出的段落，不凭记忆补原著别处的话。
- 每张卡先用一两句「原文」把作者自己的话摆出来，再用「解释」说清楚；解释尽量挂到段落号上。
- 中文写解释，原文句保持英文原样。

## 步骤 concepts
找出这一章里作者用到、并且对理解本章要紧的概念（一般 4–8 个）。每个概念一张概念卡：
- 标题写「英文词 / 中文词」；
- 至少一句原文：作者定义或第一次说清这个概念的那句；
- 一两句解释：这个概念在本章里是什么意思、和日常用法有什么不同。

## 步骤 arguments
找出这一章的主要论证（一般 2–4 个）。每个论证一张论证卡：
- 标题用一句话概括结论；
- 前提和结论各用原文句摆出来（按作者的顺序），中间用解释把推理的步骤连起来；
- 例子（如制针业）用原文句引出来，再解释它支持的是哪一步。

## 步骤 relations
给已有的卡片画关系：哪个概念是哪个论证的前提、哪个论证包含哪个例子、哪两个说法对立……
- 只连已有的卡片（用卡片编号 K1…），不新建卡片；
- 每条关系一句说明，说明也要分类别；能挂段落号就挂。
