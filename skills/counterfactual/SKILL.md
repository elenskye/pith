---
name: counterfactual
title: 假设推演
version: 1
description: 改一个前提，推演会怎样；结果标明是推演，不是原著观点
entries: [card, selection]
produces: [counterfactual, relation]
steps:
  - id: speculate
    kind: model
    title: 推演
    produces: [counterfactual, relation]
    input: [cards, scope, translation, instruction]
    max_output_tokens: 8000
---

# 假设推演

## 总则
读者选了一张论证卡或一段原文，可能还写了想改的前提。推演「如果这个前提不成立 / 换成别的，会怎样」。
- 推演的每一句都是「推演」类，不是作者的观点；
- 被改掉的那个前提，先用一句「推演」类的句子写出来，并用 `quote` 逐字引出原文里的那句前提（引文会被当原文核对）；
- 推理的每一步说清依据：依据原文的另写一句原文句；依据常识或史实的写成解释。

## 步骤 speculate
- 一般出 1–3 张推演卡：标题写「如果……」；
- 每张推演卡和它改动的那张论证卡（有的话）画一条「前提」或「修正」关系。
读者没写要改什么时，挑论证里最关键、最值得怀疑的那个前提。
