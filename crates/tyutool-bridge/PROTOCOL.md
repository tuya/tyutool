# tyutool-bridge 本地 WS 协议（实现版契约）

> 服务端：`ws://127.0.0.1:18730`（编译期常量，被占用即启动失败退出码非 0，不漂移）。
> JSON 文本帧，`type` 区分（snake_case）；请求帧带 `request_id`，应答/推送帧以同 `request_id` 关联。
> 本文件描述 **crates/tyutool-bridge 已实现** 的帧（切片 B1~B5），是前端 BridgeChannel 编解码的对账依据；
> 与《071 技术方案》样例的差异点在文末单列。协议蓝本参考 tyutool-cli serve，但消息类型独立、不兼容。

## 握手

- WS 升级阶段校验 `Origin` 头：必须与编译期白名单**逐字节相等**，否则 HTTP 403 拒绝并断开。
- **缺失 Origin 一律拒绝**（视为非浏览器来源）。
- 当前白名单（生产域名清单联调期确认后补充，见 `ORIGIN_ALLOWLIST`）：
  `http://localhost:3000` / `http://127.0.0.1:3000` / `http://localhost:5173` / `http://127.0.0.1:5173`
- 无鉴权（PRD 口径）；仅监听 127.0.0.1。

## 连接建立后服务端立即推送（顺序保证）

### hello（B1）

```json
{ "type": "hello", "app_version": "0.1.0", "protocol_version": 1, "platform": "darwin", "os_version": "26.5" }
```

| 字段 | 说明 |
| --- | --- |
| `app_version` | bridge crate 版本（与 OSS release.json 比对做新版提示） |
| `protocol_version` | 整数，当前 `1`；破坏性变更才 +1（Web 端兼容门） |
| `platform` | `darwin` / `windows` / `linux`（编译期定） |
| `os_version` | 真实系统版本（macOS `sw_vers`；Linux os-release VERSION_ID→uname -r；Windows `ver` 解析；兜底 `"unknown"`） |

### ports（B2；连接即推全量，此后变更推送）

```json
{ "type": "ports", "ports": [ { "port": "/dev/tty.wchusbserial56D70347441", "vid": "1A86", "pid": "55D2", "vendor": "WCH", "whitelisted": true, "busy": false, "first_seen_ms": 1784800000000 } ] }
```

- **永远是全量列表，不是增量**；后台 1s 枚举 diff，有变更才推，广播到所有连接。
- `vid` / `pid`：大写 4 位 hex 字符串；非 USB 串口**省略字段**（不是 null）。`vendor` 同理可省略。
- `vendor`：按 VID 常量映射（`1A86`→`WCH`、`10C4`→`Silicon Labs`、`0403`→`FTDI`），未知 VID 无此字段。
- `whitelisted`：VID ∈ {0x1A86, 0x10C4, 0x0403}；非白名单设备**照常推送**、值为 false（前端置灰）。
- `busy`：枚举源占用 **或** bridge 自身任务持有（烧录/授权在途即 true）；claim/release 即触发一次推送。
- `first_seen_ms`：毫秒时间戳；设备在场期间稳定；**拔掉重插按新设备重新计时**。

## 任务类请求（Web → Bridge）

统一应答形态：0..n 帧 `progress` + 恰好 1 帧终态 `job_result`（同 `request_id`）。

### run_job（B3，固件烧录）

```json
{ "type": "run_job", "request_id": "j-001",
  "job": { "chip_id": "t5ai", "port": "/dev/tty.xxx", "baud_rate": 2000000, "mode": "write", "start_addr": 0 },
  "file_content": "<base64>" }
```

- `mode`：可省略，默认 `"write"`；当前仅支持 `"write"`（映射 tyutool-core `FlashMode::Flash`），其他值 → `job_result bad_request`。
- `start_addr`：可省略，默认 0（数字，非 hex 字符串）。
- `file_content`：base64 固件，解码失败 → `bad_request`。
- 串口仲裁：全局注册表 port→持有任务；已被持有 → **立即** `job_result port_busy`，不排队。

### run_auth（B4，授权码写入）

```json
{ "type": "run_auth", "request_id": "a-001",
  "auth": { "port": "/dev/tty.xxx", "chip_id": "t5ai", "uuid": "uuidxxxxxxxx", "auth_key": "keyxxxxxxxxxxxxxxxx", "baud_rate": 921600 } }
```

- `uuid` / `auth_key` 为空或缺失 → `job_result bad_request`（不触达执行层、不占串口）。
- `baud_rate` 可省略，默认 921600。
- 存储位与冲突策略：`AuthStorage::Kv`（默认位）+ `ConflictPolicy::Overwrite`（对齐 PRD「自定义授权码覆盖不可撤销」警示口径）；wire 暂无 storage 字段，需要 OTP 时再扩展。
- 与 run_job 共用同一仲裁表：同串口互斥双向成立。

### cancel

```json
{ "type": "cancel", "request_id": "j-001" }
```

- 置协作取消标志；任务以 `job_result ok=false error_code="cancelled"` 终结并释放串口。
- **作用域=发起任务的连接**（request_id 按连接命名空间隔离，跨连接不可取消他人任务）。
- 未命中（任务不存在/已结束）：静默忽略（log 记录，无应答帧）。
- 连接断开时该连接的在途任务自动 cancel（关标签页不会永久占串口）。

### check_port

```json
{ "type": "check_port", "port": "/dev/tty.xxx" }
```

```json
{ "type": "check_port_result", "port": "/dev/tty.xxx", "available": false, "reason": "occupied_by_bridge_job" }
```

- bridge 自身持有 → `reason: "occupied_by_bridge_job"`（不做 OS 探测）。
- 否则 OS 级探测（tyutool-core `check_port_available`，实际尝试打开）：不可用 → `reason: "occupied_by_other_process"`；可用 → `available: true` 且 **省略 reason 字段**。
- 探测任务本身失败（阻塞线程 join 失败等内部错误）→ `available: false, reason: "probe_failed"`（罕见兜底分支）。
- **reason 取值全集：`occupied_by_bridge_job` / `occupied_by_other_process` / `probe_failed`**；前端遇到未识别的 reason 按「占用」处理即可。
- ⚠ 不存在的串口也报 `occupied_by_other_process`（core 不区分 not_found；细分粒度联调期再定）。

## 串口监视器（B5，serial_debug_*）

**一条连接同一时刻至多一个会话**，因此这组帧**不带 `request_id`**（与任务帧不同：一条连接可并发多个任务）。
会话与烧录/授权**共用同一张仲裁表**：会话持有的串口，别人既开不了会话也跑不了任务，反之亦然。

### serial_debug_open（Web → Bridge）

```json
{ "type": "serial_debug_open",
  "cfg": { "port": "/dev/tty.xxx", "baud_rate": 115200, "data_bits": 8, "stop_bits": 1, "parity": "none" } }
```

- 字段全 snake_case、位宽用**数字**（刻意不复用 core `DebugConfig` 的 camelCase + 拼写枚举 `"dataBits":"eight"`）。
- 除 `port` 外全部可省略，默认：`baud_rate` 115200、`data_bits` 8、`stop_bits` 1、`parity` `"none"`。
- 取值映射 core：`data_bits` 5/6/7/8；`stop_bits` **仅 1/2**（core 另有 1.5，但 serialport 无法真正设置，故 wire 不提供）；`parity` `none`/`odd`/`even`。
- `port` 缺失/空串、或上述任一取值非法 → `serial_debug_open_failed error_code:"bad_request"`（不占串口）。

成功：

```json
{ "type": "serial_debug_opened" }
```

失败：

```json
{ "type": "serial_debug_open_failed", "error_code": "port_busy", "message": "..." }
```

| error_code | 触发 |
| --- | --- |
| `bad_request` | port 缺失/空、data_bits·stop_bits·parity 取值非法 |
| `port_busy` | 该串口被其他任务/会话持有，或处于**他人的烧后交接窗口**内 |
| `already_open` | 本连接已有会话（不静默替换：替换会让在跑会话的串口占用悬空） |
| `open_failed` | 执行层打开串口失败（message = core 错误文本） |
| `internal` | 打开线程 join 失败（panic 等） |
| `unsupported` | backend 不支持串口监视（正常生产路径不出现） |

### serial_debug_chunk_batch（Bridge → Web）

```json
{ "type": "serial_debug_chunk_batch",
  "chunks": [ { "ts_ms": 1784800000042, "direction": "rx", "bytes_b64": "Ym9vdA==" } ] }
```

- **批量帧**：12ms / 32KiB 双阈值节流（与 tyutool-serve、GUI 同参数），一次串口突发合并成一帧，避免高频小帧撑爆连接的有界发送队列（256 帧满即判定客户端失活断开）。
- `direction`：`"rx"` / `"tx"` 小写。
- `bytes_b64`：base64（core 的 `DebugChunk.bytes` 是 JSON 数字数组，wire 换成 base64，体积小一个数量级且与 `file_content` 同一套解码）。
- B5 只有设备→主机方向；`serial_debug_send` / 归档 / 过滤器 / device_reset 等 serve 侧能力**未实现**。

### serial_debug_close（Web → Bridge）

```json
{ "type": "serial_debug_close" }
```

```json
{ "type": "serial_debug_closed" }
```

- **幂等**：没有会话时也回 `serial_debug_closed`（与 serve 蓝本一致，前端拆卸路径无需判分支）。
- 时序保证：先关会话 → 冲净残余 chunk_batch → 释放串口 → 才发 `serial_debug_closed`。
  收到该帧即可立刻重开同一串口，不会撞上尚未释放的旧句柄。

### serial_debug_disconnected（Bridge → Web，推送）

```json
{ "type": "serial_debug_disconnected", "reason": "device_removed" }
```

- 设备侧断链（拔线、驱动报错）；`reason` 为执行层文本，原样透传。
- 与 `serial_debug_closed` 同样的时序保证：**释放串口后**才推该帧，收到即可重开。
- 与用户主动 close 竞态安全：会话槽位只能被取走一次，谁先取到谁负责关闭与释放，另一方自动降级为 no-op（因此断链后不会再来一帧 `serial_debug_closed`，反之亦然）。
- 该保证不限于「打开在途」：**断链一取走会话就开始吞**，不必等 `serial_debug_disconnected` 真正发出——拆会话、释放串口这段窗口内到的 close 也一样（会话已打开时断链、打开在途断链、或该次打开最终失败后补发，三条路径同理），那条**已在路上的 close 就被吞掉**（不回帧）——同一个会话只有一帧终态。
- 吞掉是**一次性**的：再来一次 close 就是「没有会话」的新拆卸，仍按 §serial_debug_close 的幂等回 `serial_debug_closed`（前端连发两次 close 不会挂死等帧）。
- 若 close / 断链发生在**打开在途**（后端 open 尚未返回）期间：两方都先静默，等该会话被拆掉、串口释放后才发终态帧，且**恰好一帧**——由先取到槽位的那方决定是 `serial_debug_closed` 还是 `serial_debug_disconnected`。打开最终失败时先回 `serial_debug_open_failed`，再补这一帧。
- 连接断开时若仍有会话，bridge 自动关闭并释放串口（关标签页不会永久占串口）。

### 烧后交接窗口（handoff window）

**任一烧录/授权作业**（`run_job` 或 `run_auth`）**成功**（`job_result ok=true`）后，该串口不是立即完全释放，而是降级为**为发起连接预留 3 秒**：

- 预留期内：**其他连接** 的 `serial_debug_open` / `run_job` / `run_auth` 一律 `port_busy`；
- 预留期内：**发起该作业的那条连接** 的任意 claim 立即成功并接管预留（原子交接，无需抢锁重试）；
- 3 秒后（惰性判定：只在下一次 claim 时检查过期）任何连接都可正常占用；
- 失败 / 取消的任务**不**预留，立即完全释放；
- 发起连接断开时，其名下所有预留立即清除。

动机：「烧完/授权完立刻看启动日志」是主路径，而 `job_result` 与客户端 `serial_debug_open` 之间存在一个真空窗口，别的标签页可能把刚准备好的板子抢走。前端「固件+授权」连跑以 `run_auth` 收尾，预留归属那次授权作业的连接。

⚠ **预留态在 `ports` 帧里 `busy=false`**：`busy` 表示「有人正在驱动这个串口」，B3 已定契约是 `job_result` 后 `busy` 立刻翻 false。预留是短暂门禁态而非占用态，故不体现在设备列表。
代价是：窗口内被拒的 `port_busy` 在设备列表里找不到对应的 `busy=true`——这比每次烧录都让整张列表 true→false→true 闪一下更可接受。
`check_port` 同理：预留态不算 bridge 自持，会落到 OS 级探测。

## 任务应答帧（Bridge → Web）

### progress

```json
{ "type": "progress", "request_id": "j-001", "payload": { ... } }
```

`payload` 恒为 JSON 对象：
- run_job：tyutool-core `FlashEvent` 的 serde JSON **原样透传**（与 tyutool-cli serve 的 progress payload 同源同词汇——kind 标签，如 JobSummary/Phase/Percent/Milestone/Done）。技术方案样例 `{"phase","percent","log"}` 为示意，实际以 FlashEvent 序列化为准。
- run_auth：`{"step": "<BatchAuthStep>"}`——core 的 `BatchAuthStep` 序列化为裸 snake_case 字符串（`reading_mac` / `reading_auth` / `writing_auth` / `verifying` …），bridge 包一层 `step` 键使 payload 保持对象形，前端按 request 种类无需特判非对象 payload。

### job_result（终态，恰好一帧）

```json
{ "type": "job_result", "request_id": "j-001", "ok": true, "elapsed_ms": 91234 }
```

```json
{ "type": "job_result", "request_id": "j-001", "ok": false, "elapsed_ms": 1234, "error_code": "port_busy", "message": "..." }
```

**error_code 清单（稳定机器码，前端 flashErrorClassifier 的映射输入）：**

| error_code | 触发 |
| --- | --- |
| `port_busy` | 串口已被其他在途任务持有（仲裁拒绝，立即返回） |
| `cancelled` | cancel 帧 / 连接断开 / core 返回 FlashError::Cancelled |
| `bad_request` | base64 解码失败 / mode 不支持 / uuid·auth_key 为空缺失 / 同连接 request_id 重复在途 |
| `flash_failed` | run_job 执行层其余错误（message=FlashError 文本） |
| `auth_failed` | run_auth 执行层其余错误 |
| `internal` | 执行线程 join 失败（panic 等） |
| `unsupported` | backend 不支持该操作（正常生产路径不出现） |

## 通用约定

- 解析失败或未知 `type`：log 后**丢弃**，无 error 帧（与 serve 蓝本不同；协议收敛后再评估是否补 error 帧）。
- 多连接：ports 推送广播到所有连接；progress/job_result/check_port_result/serial_debug_* 只回发起连接。
- 同串口执行权全局唯一（跨连接互斥，任务与会话共表）；request_id 命名空间按连接隔离（不同连接可用相同 request_id）。
- `cancel` 只作用于任务：会话不在 request 索引里，取消帧碰不到串口监视器。
- 帧序：单连接内 hello → ports(初始) 保证有序；任务帧因果有序（progress 先于 job_result；busy=true 的 ports 推送不早于该任务首个可观察帧，busy=false 不早于 job_result）。

## 与《071 技术方案》样例的差异点（联调对账用）

1. `run_job.job.mode` 样例为 `"write"` —— 实现接受 `"write"`（默认）并映射 core `FlashMode::Flash`；`erase`/`read` 未实现（后续切片）。
2. progress payload 样例 `{"phase","percent","log"}` 为示意 —— 实际为 FlashEvent / BatchAuthStep 原样序列化（见上）。
3. `check_port_result.reason` 样例只出现 `occupied_by_other_process` —— 实现另有 `occupied_by_bridge_job`（bridge 自持）；不存在的串口暂也归 `occupied_by_other_process`。
4. `cancel` 样例未说明作用域 —— 实现为连接内命名空间（跨连接不可 cancel）。
5. run_auth 样例无存储位字段 —— 实现默认 Kv + Overwrite，OTP 需扩展 wire 字段。
6. `serial_debug_*` 帧已实现（B5），但**取子集**：open / chunk_batch（批量，非逐条 chunk）/ close / disconnected 四类，均**不带 request_id**；`cfg` 为 snake_case + 数字位宽（非 core 的 camelCase 拼写枚举）；`serial_debug_send`、归档回放、关键字过滤器、device_reset 等 serve 侧能力未实现。另新增技术方案未描述的**烧后交接窗口**（见上节），其预留态刻意不体现在 `ports.busy`。
7. hello 后固定紧跟一帧全量 ports —— 技术方案「连接即推全量」的落地形态（顺序保证写死）。
