# 夹具：《国富论》第一篇第 1–3 章

| 文件 | 来源 | 说明 |
| --- | --- | --- |
| `en-book1-ch1-3.txt` | Project Gutenberg #3300《An Inquiry into the Nature and Causes of the Wealth of Nations》（Adam Smith，公有领域；Credits: Colin Muir and David Widger），2026-11-13 下载 | 只取第一篇的篇名和第 1–3 章正文；去掉了 Project Gutenberg 的商标、许可说明和全部页眉页脚。原文一个字没改，只按 Glasgow 版的分段把 PG 本合在一起的段落拆开（第 1–3 章 = 11 / 5 / 8 段） |
| `zh-synthetic.txt` | 为测试编写的合成中文片段 | **不是任何真实译本的文字**。故意带：目录（前后附）、译者导言、前三章导读（导读里也有注释）、旁注（短 `blockquote`）、上标的注释引用和章末注释、接在注释后面的「译者按」；第三章只有 7 段（和英文的 8 段对不上，测「不对齐」） |

**为什么不是 Cannan 版**：任务要的是 Cannan 1904 版。Project Gutenberg 上没有 Cannan 版（只有 #3300 和带 Garnier 注的 #38194），Online Library of Liberty 拒绝自动下载（CloudFront 403），所以用 PG #3300。它的正文和 Cannan 版同出第 5 版，个别拼写和分段不同（PG 本 `judgment`，Penguin 版 `judgement`）；Cannan 版的页边提要没有，改由中文合成夹具测旁注。

EPUB 不放二进制：`src/fixture.rs` 现拼（英文 EPUB 3 / EPUB 2 两种，加合成的目录、编者导言、注释和每约 1500 字一个的假分页点，第一篇从第 101 页起；中文 EPUB 2）。
