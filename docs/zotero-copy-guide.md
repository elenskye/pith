# 导出 Zotero 库副本（给用户）

目的：让 Agent 在**不触碰** `~/Zotero/` 的前提下，用你真实库的副本做 Phase 0 验证和 v0.1 验收。副本放项目外，只读，不进 Git。

## 步骤

1. **完全退出 Zotero**（⌘Q，确认菜单栏没有 Zotero 图标）。Zotero 运行时数据库被独占锁定，复制会不完整。

2. 建目录：

```bash
mkdir -p ~/ResearchLab-test/zotero-copy
```

3. 用 SQLite 的在线备份命令复制数据库（比 cp 安全，能保证一致性）：

```bash
sqlite3 -readonly ~/Zotero/zotero.sqlite ".backup '$HOME/ResearchLab-test/zotero-copy/zotero.sqlite'"
```

4. 复制附件目录（PDF 等都在 `storage/` 里，按条目 key 分子目录）：

```bash
rsync -a ~/Zotero/storage/ ~/ResearchLab-test/zotero-copy/storage/
```

5. 看一眼规模，把输出贴给 Agent：

```bash
du -sh ~/ResearchLab-test/zotero-copy/storage && sqlite3 -readonly ~/ResearchLab-test/zotero-copy/zotero.sqlite "select count(*) from items where itemID not in (select itemID from deletedItems);"
```

6. 之后可以重新打开 Zotero，照常使用。副本是快照，不会跟着变。

## 注意

- 如果你在 Zotero 里用过「链接文件」（Linked File，而不是存入 Zotero 的附件），那些文件不在 `storage/` 里，Agent 导入时会按路径报告「找不到」，不会出错。
- `~/Zotero/styles/`、`translators/` 不需要复制。
- Agent 对 `~/ResearchLab-test/` 只读；如果以后想刷新副本，重复 1–5 即可。
