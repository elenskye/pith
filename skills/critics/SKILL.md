---
name: critics
title: 反方与后世评论
version: 1
description: 给概念卡或论证卡找有出处的反对、修正和后世看法：先查文献库，不够再上网
entries: [card]
produces: [comment, relation]
steps:
  - id: library
    kind: library
    title: 查文献库
    max_results: 5
  - id: web
    kind: web
    title: 上网找
    max_results: 3
    optional: true
    off_by_default: true
  - id: comment
    kind: model
    title: 摘观点
    produces: [comment, relation]
    input: [cards, sources]
    max_output_tokens: 10000
---

# 反方与后世评论

## 总则
给定的卡片是这本书里的一个概念或论证；来源（S1、S2……）是文献库里或网上找到的文献正文。从来源里摘出对这个概念或论证的反对、修正和后世看法。
- 每条看法都要有出处：用「他人」类的句子逐字摘录来源原文，`source` 写来源编号；
- 来源里没有的看法不要写成「他人」。你自己想到的反驳只能写成「解释」，并在句中写明「这是解释，不是文献里的话」；
- 来源与卡片无关就不用它。

## 步骤 comment
- 每个有用的来源出一张评论卡：标题写「谁 / 哪篇：一句话概括他的看法」；先用他人句摘出关键的话，再用解释说明它是反对、修正还是发展，针对的是卡片里的哪一句；
- 给每张评论卡和它评论的那张卡片画一条关系（对立、修正或包含），卡片编号用给出的编号。
