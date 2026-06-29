# batch-auth 串口日志增强设计

**日期:** 2026-06-29
**文件:** `crates/tyutool-core/src/authorize.rs`
**范围:** 仅 `authorize.rs`，不涉及前端或其他 Rust crate

---

## 背景

批量授权工具（`run_batch_auth_slot`）目前的 `log::info!` 覆盖了高层状态转移（slot start、mac read、writing、done 等），但缺少：

- 实际发送到设备的命令字符串
- 设备回复的原始响应行
- 设备现有 auth 读取结果
- MAC / auth-read 重试的第 n 次尝试
- `detect_firmware` 返回的固件类型和版本

没有这些信息，排查授权失败时只能看到"verify-fail"，不知道具体发了什么、收到了什么。

---

## 目标

在 `debug` 日志级别补充串口收发的完整记录和关键节点，不改变 `info!` 级别的现有日志，不增加 `FlashEvent` 用户可见输出。

---

## 设计

### 方案选型

采用**方案 A：集中式**，在 `send_cmd` 和 `read_response_timed` 这两个最底层的 I/O 函数中加日志，一次性覆盖所有现有和未来命令，无需逐函数修改。

boot 探测阶段（`sys_log_enable off` 轮询）会产生少量探测日志，但 T5AI 实测通常 1–2 次即探测成功，debug 级别下噪音可接受。

### 变更一：`AuthSession` 新增 `port_name` 字段

```rust
struct AuthSession<T: AuthIo> {
    port: T,
    timing: AuthTiming,
    port_name: String,   // 新增，用于 debug 日志
}
```

`open()` 中赋值：
```rust
Ok(Self {
    port: SerialAuthIo { port },
    timing,
    port_name: port_name.to_string(),
})
```

测试用 `MockAuthIo` 的构造函数传空字符串，无需改动已有测试。

### 变更二：`send_cmd` — 发送日志

在写串口之前加一行：

```rust
fn send_cmd(&mut self, cmd: &str) -> Result<(), FlashError> {
    log::debug!("[serial] port={} >> {}", self.port_name, cmd);
    let _ = self.port.clear_input();
    // ...其余不变
}
```

覆盖范围：`read_mac`（`read_mac`）、`auth_read`（`auth-read` / `auth-read N`）、`auth_write`（`auth UUID KEY` / `auth UUID KEY N`）、`auth_otp_lock`（`auth-otp-lock`）、`detect_firmware` 探测（`sys_log_enable off`、`version`）。authkey 完整记录，不脱敏。

### 变更三：`read_response_timed` — 接收日志

在函数末尾 return 前记录：

```rust
log::debug!(
    "[serial] port={} << {:?} ({}ms)",
    self.port_name,
    lines,
    fn_start.elapsed().as_millis()
);
lines
```

包含收到的所有行（已去 ANSI、已 trim）和本次等待总耗时（毫秒）。

### 变更四：`run_batch_auth_slot` — 补充关键节点

以下四处均使用 `log::debug!`，不升级为 `info!`：

| 位置 | 日志格式 |
|------|---------|
| `detect_firmware` 返回后 | `[batch-auth] firmware  port={} kind={:?}` |
| `existing_auth` 读取后（新固件路径） | `[batch-auth] existing-auth  port={} mac={} result={}` |
| MAC read 每次重试（第 2 次起） | `[batch-auth] mac-read retry {i}/{total}  port={}` |
| auth-read 每次重试（第 2 次起） | `[batch-auth] auth-read retry {i}/{total}  port={}` |

`existing-auth result` 格式：有值时为 `uuid=<uuid>`，无值时为 `none`。

旧固件路径（`FirmwareKind::Old`）同样补充 `existing_auth` 读取结果和重试日志，保持与新固件路径对称。

---

## 日志示例（T5AI 正常授权流）

```
DEBUG [serial] port=/dev/ttyACM0 >> sys_log_enable off
DEBUG [serial] port=/dev/ttyACM0 << ["OK: log disabled", "tuya>"] (12ms)
DEBUG [batch-auth] firmware  port=/dev/ttyACM0 kind=New(10004)
DEBUG [serial] port=/dev/ttyACM0 >> read_mac
DEBUG [serial] port=/dev/ttyACM0 << ["AA:BB:CC:DD:EE:FF", "tuya>"] (18ms)
 INFO [batch-auth] read mac  port=/dev/ttyACM0 mac=AA:BB:CC:DD:EE:FF
DEBUG [serial] port=/dev/ttyACM0 >> auth-read
DEBUG [serial] port=/dev/ttyACM0 << ["00000000000000000000000000000000", "0000....", "tuya>"] (22ms)
DEBUG [batch-auth] existing-auth  port=/dev/ttyACM0 mac=AA:BB:CC:DD:EE:FF result=uuid=00000000000000000000000000000000
 INFO [batch-auth] allocated  port=/dev/ttyACM0 mac=AA:BB:CC:DD:EE:FF uuid=<uuid>
 INFO [batch-auth] writing  port=/dev/ttyACM0 mac=AA:BB:CC:DD:EE:FF uuid=<uuid>
DEBUG [serial] port=/dev/ttyACM0 >> auth <uuid> <authkey>
DEBUG [serial] port=/dev/ttyACM0 << ["Authorization write succeeds.", "tuya>"] (45ms)
DEBUG [serial] port=/dev/ttyACM0 >> auth-read
DEBUG [serial] port=/dev/ttyACM0 << ["<uuid>", "<authkey>", "tuya>"] (20ms)
 INFO [batch-auth] verify ok  port=/dev/ttyACM0 mac=AA:BB:CC:DD:EE:FF uuid=<uuid>
 INFO [batch-auth] done  port=/dev/ttyACM0 mac=AA:BB:CC:DD:EE:FF uuid=<uuid>
```

---

## 不在本次范围内

- `FlashEvent` 用户可见输出（不变）
- `drain_boot_output` 内部（丢弃的字节数已够，不需要逐字节记录）
- `wake_shell`（只发 `\r\n`，无业务语义）
- 前端日志展示
- 其他 crate（`tyutool-cli`、`src-tauri`）

---

## 测试影响

- 现有 Rust 单元测试（164 个）使用 `MockAuthIo`，`port_name` 初始化为空字符串，无功能性变化
- `send_cmd_writes_command_with_crlf` 等测试直接断言串口写入内容，不受日志影响
- 无需新增测试（日志路径无业务逻辑）
