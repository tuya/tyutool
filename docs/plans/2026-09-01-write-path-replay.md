# 写入路径录制回放 实施计划

**Spec:** `docs/specs/2026-09-01-write-path-replay-design.md`

**Goal:** 给 Beken 驱动的写入路径（解保护 → 擦除 → 写 → 每扇区 CRC → 保护）补一条来自真实
硬件的协议基线，并把上一轮遗留的另外两个缺口各自定性收口。

**Architecture:** 复用上一轮已有的 `RecordIo` / `ReplayIo` 与线格式，不新增机制。只多一份
fixture 和一个回放测试。

**Tech Stack:** Rust 2021。回放测试跑在**普通** `cargo test -p tyutool-core` 里
（`#[cfg(test)]`，不带 feature）；录制需要 `record-io` feature 和真实硬件。

**基线（`869fcb1`）：** core 322 / cli 59 / serve 31 / tauri 36。

---

## File Map

| File | Change |
|---|---|
| `crates/tyutool-core/src/plugins/beken/t5ai-write-16k.trace` | 新 fixture |
| `crates/tyutool-core/src/plugins/beken/driver.rs` | `mod replay` 下新增写入回放测试 |
| `docs/specs/2026-09-01-write-path-replay-design.md` | 本对文档 |
| `docs/plans/2026-09-01-write-path-replay.md` | |

---

## Task 1: 安全性前置检查

- [x] **Step 1:** 确认擦除粒度：`ops.rs` 的 `use_block_erase` 要求 `remaining >= 64 KiB`，
  16 KiB 写入只走 4 KiB 扇区擦除，擦除范围 == 写入范围
- [x] **Step 2:** 切出固件前 16 KiB 作为写入源——写回设备上本来就有的字节，内容净变化为零
- [x] **Step 3:** 写入**之前**读一次 16 KiB，与固件头部逐字节比对，确认起点状态

## Task 2: 实机录制

- [x] **Step 1:** 带 `--features tyutool-core/record-io` 重新构建 Windows CLI
  （不复用几小时前的二进制——录制器必须与回放它的代码同源）
- [x] **Step 2:** 录制 `write -s 0x0 -f <16 KiB>`
- [x] **Step 3:** 写入**之后**再读一次 16 KiB，与固件头部逐字节比对，确认内容零变化

## Task 3: 回放测试

- [x] **Step 1:** fixture 落 `plugins/beken/t5ai-write-16k.trace`，头部写明出处、设备、命令、
  以及「内容零变化」这一安全性依据
- [x] **Step 2:** 回放测试驱动 `run_beken_on_transport`，Flash 模式，断言成功且
  `remaining() == 0`
- [x] **Step 3:** ⚠ **金标准阴性验证**：改 fixture 里一条写入的一个字节 → 必须失败；还原

## Task 4: 门禁

- [x] **Step 1:** 全量门禁（fmt / clippy ×3 / 全部测试 / bindings）
- [x] **Step 2:** cli / serve / tauri 计数**不变**；core 增加

---

## 验收标准

- 回放测试在**普通** `cargo test -p tyutool-core` 里跑
- 金标准经过阴性验证
- 录制前后两次读取都与固件头部一致——写入是内容零变化的，有据可查
- 另外两个缺口在 spec 里各自定性（重新定义 / 明确不做），不再是开放项
