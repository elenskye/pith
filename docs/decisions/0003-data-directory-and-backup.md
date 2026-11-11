# 0003 · 数据目录、备份与恢复

- 日期：2026-11-07
- 状态：已定（问卷 Q18 + Pomelo `docs/architecture.md` 做法）

## 决定

```
~/Pith/                        正式数据目录（用户 Q18：一个目录装下全部；原 ~/ResearchLab/，0014 改名）
├── library.db                 主库（SQLite，编号 migration）
├── search.db                  FTS 索引，可删除重建，不进备份
├── attachments/<sha256前2>/<sha256>.<ext>   附件按内容哈希去重存放
├── backups/library-YYYYMMDD-HHMMSS-<tag>.db  tag: daily / manual / before-import / before-upgrade / before-restore
└── exports/                   整库导出（目录或 zip）

~/Pith Dev/                    开发版数据目录（`pnpm app:dev` 用）
<temp>/researchlab-test-<pid>-<name>/   Rust 测试临时目录
```

- 启动时：每天第一次打开自动备份；升级 migration 前备份；导入前备份；恢复前备份。daily 保留 30 份，更早每月留一份。
- 数据库 `user_version` 比 App 新 → 拒绝打开并提示。
- 整库导出 = `library.json`（全部表，格式版本 1）+ `attachments/`；恢复时核对摘要与哈希（Phase 6 定稿，`0013`；原计划的 `library.db` + 清单改成了 JSON，理由见 0013）。
- 恢复（备份或导出）先做 `before-restore` 备份；还有备份用到的附件文件不删（`0013`）。
- 附件原始文件名、原始路径、导入来源记在 DB，文件本体只按哈希存一份。
- 开发与测试**绝不**触碰 `~/Pith/`。

## 不做（v0.1）

- iCloud / 同步盘目录（SQLite 与同步盘冲突）。
- 撤销系统（Pomelo 的 session 扩展撤销）：v0.1 用「导入前备份 + 事务」兜底，撤销放 v0.1.x 评估。
