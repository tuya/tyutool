# tyutool-bridge 本地 WS 协议（实现版契约）

> 服务端：`ws://127.0.0.1:18730`（编译期常量，被占用即启动失败退出码非 0，不漂移）。
> JSON 文本帧，`type` 区分（snake_case）；请求帧带 `request_id`，应答/推送帧以同 `request_id` 关联。
> 本文件描述 **crates/tyutool-bridge 已实现** 的帧（切片 B1~B7），是前端 BridgeChannel 编解码的对账依据；
> 与《071 技术方案》样例的差异点在文末单列。协议蓝本参考 tyutool-cli serve，但消息类型独立、不兼容。

## 握手

- 仅监听 127.0.0.1。
- WS 升级阶段校验 `Origin` 头：必须与编译期白名单**逐字节相等**，否则 HTTP 403 拒绝并断开。
- **缺失 Origin 一律拒绝**（视为非浏览器来源）。
- 当前白名单共 **18 条**（见 `ORIGIN_ALLOWLIST`，来源：cobuilder-web `config/index.cjs` 的
  `base` / `daily` / `pre` / `prod` 区域表）：
  - 本地开发（4）：`http://localhost:3000` / `http://127.0.0.1:3000` / `http://localhost:5173` / `http://127.0.0.1:5173`
  - 日常（1）：`https://dev-claw-wb.wgine.com`（`base` 与 `daily` 两个块同指它，去重后 1 条；
    团队在该环境验证烧录，必须随包发出）
  - 预发（wgine，6）：`https://developer.wgine.com` / `https://developer-us.wgine.com` /
    `https://developer-eu.wgine.com` / `https://developer-in.wgine.com` /
    `https://developer-ue.wgine.com` / `https://developer-we.wgine.com`
    （预发 AZ 与 SG 同指 `developer-us`，去重后 6 条）
  - 生产（tuya，7）：`https://platform.tuya.com` / `https://us.platform.tuya.com` /
    `https://eu.platform.tuya.com` / `https://ind.platform.tuya.com` /
    `https://ue.platform.tuya.com` / `https://we.platform.tuya.com` / `https://sg.platform.tuya.com`
    （生产 SG 是独立域名 `sg.platform.tuya.com`，不与 AZ 合并）
- ⚠ **漏一个域名 = 重发安装包 + 存量用户全部重装**：白名单是编译期常量，随二进制发出，
  改配置文件救不回来；漏掉的环境上第一次连 Bridge 就是 403，只能再走一轮发版。
  新环境/新区域上线前就要补进来，宁可多列一个合法的 cobuilder-web 域名。
- ⚠ 白名单**只做逐字节精确匹配，禁止改成通配/后缀匹配**（`*.wgine.com` 之类等于把烧录/授权能力
  交给任何拿下兄弟子域的人）。上面那条重装代价是"把域名列全"的理由，**不是**用通配的理由；
  新增区域只能按字面量追加。
- 握手 query 可带回上次的授权令牌：`ws://127.0.0.1:18730/?token=<token>`。
  **403 仅属于 Origin 校验**，令牌问题永不返回 403、只降级为未授权。
- ⚠ `Origin` 只是过滤器，不是信任根 —— 完整口径与令牌生命周期见 [§安全模型](#安全模型b7本地传输加固)。

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
{ "type": "ports", "ports": [ { "port": "/dev/cu.usbmodem56D70427241", "vid": "1A86", "pid": "55D2", "vendor": "WCH", "serial_number": "56D7042724", "usb_interface": 1, "whitelisted": true, "busy": false, "first_seen_ms": 1784800000000 } ] }
```

- **永远是全量列表，不是增量**；后台 1s 枚举 diff，有变更才推，广播到所有连接。
- `vid` / `pid`：大写 4 位 hex 字符串；非 USB 串口**省略字段**（不是 null）。`vendor` 同理可省略。
- `vendor`：按 VID 常量映射（`1A86`→`WCH`、`10C4`→`Silicon Labs`、`0403`→`FTDI`），未知 VID 无此字段。
- `whitelisted`：VID ∈ {0x1A86, 0x10C4, 0x0403}；非白名单设备**照常推送**、值为 false（前端置灰）。
- `serial_number`：USB iSerial 字符串，**同一物理设备的多个串口该值相同，前端据此归组**。
  拿不到（非 USB 串口、设备没烧 iSerial）时**省略字段**，与 `vid`/`pid` 同一处理方式。
- `usb_interface`：USB 接口号（数字），用来区分 `serial_number` 相同的多个口。
  **拿不到是常态、不是异常**（Linux 经常不报），此时**省略字段**——消费方不得把「缺失」当错误处理。

#### 一个设备两个口：为什么必须由前端归组

T5 这类板子是**双串口**的：一块板子的 UART 桥在系统里就是两个串口设备。实测（`tyutool-cli usb-port-survey`）：

```
/dev/cu.usbmodem56D70427241  vid 1A86 pid 55D2  serial_number "56D7042724"  usb_interface 1
/dev/cu.usbmodem56D70427243  vid 1A86 pid 55D2  serial_number "56D7042724"  usb_interface 3
```

两行 `serial_number` 完全相同，只有 `usb_interface` 不同。**Bridge 只如实上报这两个字段，
不在服务端归组、不排序、不标记「推荐端口」**：ports 帧的单位永远是**端口**而不是设备
（改成推设备是破坏性变更，而且前端仍然需要能选到具体端口）。归组逻辑放前端。

⚠ **`usb_interface` 的取值不可跨平台比较**（与 tyutool Tauri 端 `src/utils/serial-port-label.ts` 同一口径）：

| 平台 | 烧录/授权口 | 日志口 |
| --- | --- | --- |
| macOS | 1（CDC data 接口） | 3 |
| Windows | 0（配对的 control 接口） | 2 |
| Linux | 常常**不报** `usb_interface` | 同左 |

所以判定要按 `{0,1}` / `{2,3}` 两个集合来写，不能写死某个数字；缺失时只能退化成
「这是个双串口设备」的通用提示。

⚠ **这是「提示」，不是权威判定。** tyutool Tauri 端的 i18n 键刻意叫
`flash.tuyaPortHint.maybeFlashAuth` / `maybeLog`——用的是「可能是」的口吻，
它的自动选口逻辑（`useFlashConnection.ts`）其实只取 `ports[0].path`，并不依赖角色推断。
Bridge 前端请保持一致：角色只用于 hover 提示帮用户判断，**不要拿它做自动烧录决策**。
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

- `chip_id` / `port` / `baud_rate`：**三个都必填**。
- ⚠ **`baud_rate` 必填，与 run_auth / serial_debug_open（两者都默认 115200）不同** ——
  这是刻意的不对称，**不要为了「一致性」顺手给它加 `#[serde(default)]`**。
  理由：烧录波特率是会**影响写入过程**的参数（T5 用 2000000），静默套一个默认值可能让烧录变慢、
  甚至中途失败，而那种失败排查起来极其昂贵；**明确报错比猜一个值更安全**。
  run_auth / serial_debug_open 的波特率只影响串口会话本身，猜错了大不了读到乱码，代价不对称。
  省略该字段 → `job_result bad_request`（见 §通用约定 的不可解析帧处理）。
- `mode`：可省略，默认 `"write"`；当前仅支持 `"write"`（映射 tyutool-core `FlashMode::Flash`），其他值 → `job_result bad_request`。
- `start_addr`：可省略，默认 0（数字，非 hex 字符串）。
- `file_content`：base64 固件，解码失败 → `bad_request`。
- 串口仲裁：全局注册表 port→持有任务；已被持有 → **立即** `job_result port_busy`，不排队。
- 另有**全局单一执行权**：已有危险操作在途（含正在等确认）→ **立即** `job_result execution_busy`（见 §单一执行权）。

### run_auth（B4，授权码写入）

```json
{ "type": "run_auth", "request_id": "a-001",
  "auth": { "port": "/dev/tty.xxx", "chip_id": "t5ai", "uuid": "uuidxxxxxxxxxxxxxxxx", "auth_key": "keyxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", "baud_rate": 115200 } }
```

- `uuid` / `auth_key` **为空、缺失、或长度不合法** → `job_result bad_request`（不触达执行层、不占串口、
  **一个字节都不发给设备**，也不弹危险操作确认框——不合法的请求本来就跑不了，不该去打扰用户）。
  合法长度是**固件的硬约束**（`tuya_authorize.c`：`UUID_LENGTH=20` / `UUID_LENGTH_16=16`、
  `AUTHKEY_LENGTH=32`）：**uuid 必须是 16 或 20 个字符，auth_key 必须是 32 个字符**（先 trim）。
  规则只有一份，在 core（`tyutool_core::validate_auth_credentials`），bridge 复用它，不自己抄一遍。
  ⚠ **这类失败不是 `auth_failed`**：`auth_failed` 的含义是「已经发到设备之后出的错、设备状态未知」，
  而长度不合法是**在本机就能判定、设备完全没被碰过**，与「为空/缺失」是同一类事实，故同归 `bad_request`
  （`message` 只带**出错的长度**，永不回显 uuid / auth_key 本身）。
  背景：授权走的 core 入口 `run_authorize`（= `tyutool-cli authorize`）**自己不做这道校验**，
  真发给设备的话，畸形凭证会被固件在写入环节拒掉——白白消耗一枚后端已分配的授权码，
  还报成「设备侧失败」。批量流水线 `run_batch_auth_slot` 一直有这道写前校验，本路径补齐。
- `baud_rate` 可省略，默认 **115200**。
  ⚠ **这是固件 UART 控制台的速率，不是烧录波特率，别为了「和 run_job 一致」把它改回 921600。**
  授权走的是设备固件起来之后的 shell（`tuya>`），不是烧录 bootloader；全芯片的控制台都跑 115200
  （前端 `src/features/firmware-flash/chip-manifests.ts` 每一项的 `defaultAuthBaudRate`、
  GUI 批量授权、`tyutool-cli authorize`、web 直连 vendor 四处同值）。
  历史教训：这里曾误写成 921600（T5AI 的**烧录**波特率），而 web 端刻意省略该字段吃默认值，
  于是所有浏览器侧授权都在拿 921600 敲 115200 的控制台——设备侧全是帧错误、整窗口 `bytes=0` 零应答。
  2026-07-31 真机实测：同一块 T5AI 板子，115200 复位后 0.6s 拿到 `tuya>`、2.7s 读完授权。
  **该字段是这次授权全过程的唯一速率来源**：自然启动等待（`wait_after_firmware_flash`）和授权会话
  本身（core `run_authorize` 用 `job.baud_rate` 开串口）都取它，客户端显式传非默认值时两段也保持一致。
  （`run_authorize` 曾把会话速率写死成 115200 常量，与前半段的 `baud_rate` 分家；已收敛。）
- 存储位与冲突策略：KV（`run_authorize` 对单设备写死 KV，OTP 只属于批量流程）+ 覆盖既有凭证
  （对齐 PRD「自定义授权码覆盖不可撤销」警示口径：危险操作确认框已经问过用户）；
  wire 暂无 storage 字段，需要 OTP 时再扩展。
- 与 run_job 共用同一仲裁表：同串口互斥双向成立；且与 run_job 共用同一份全局单一执行权（见 §单一执行权）。

#### run_auth 的 core 入口（为什么不再读 MAC）

bridge 走 core 的 **`FlashMode::Authorize`**（`registry::run_job` 在查芯片插件**之前**就把它
分派给 `authorize::run_authorize`，这也是 `chip_id="other"` 能授权的原因）。语义正是本流程要的：
**把后端给定的这一对 uuid/auth_key 写进串口上的这一台设备**，与 `tyutool-cli authorize` 同一条路。

它**不读设备 MAC**。历史上 bridge 接的是 `run_batch_auth_slot`（GUI 的 Excel 批量流水线）：
那条路第一步就必须读 MAC，因为 MAC 是它去表格里查「这台设备该用哪一行凭证」的键。
CoBuilder 没有表格，适配层把查表回调写成了 `|_mac| None`，但 MAC 读取本身仍是硬前置——
于是 2026-07-31 真机上出现了「shell 625 ms 应答、固件探测成功，却报
`Failed to read MAC address`」。**`Failed to read MAC address` 从此不在 CoBuilder 链路上可达。**

#### run_auth 的自然启动等待（先等启动，再复位）

授权槽位的**第一个动作就是硬件复位**（core `detect_firmware` 脉冲 RTS）。而 Web 工作台
的典型节奏是「烧完立刻授权」，此时设备正在跑烧录后的**首次启动**——复位打断首启，首启
重来，如此往复，设备永远没机会应答。真机实测：烧完立刻授权连续三次整整 30 s **零字节**；
同一块板放几分钟等首启跑完后再跑，shell 627 / 636 / 642 ms 就应答。

因此 bridge 在进入授权槽位**之前**先调 core 的 `wait_after_firmware_flash`（GUI 批量烧录
一直是这么做的）：

- **被动读，不复位**：只读串口，唯一的写是空闲 500 ms 后发一个 `\r\n` 探针，不碰
  DTR/RTS，所以它不会打断正在进行的启动。
- **提前退出**：读到 TuyaOpen 启动横幅，或探针拿到 `tuya>` / `no command` 应答即返回。
  已经启动完的设备通常 1 s 内就放行，等待上限对它无感。
- **非致命**：开口失败或等满上限都照常往下走授权，不产生任何 error_code。
- **波特率跟 `baud_rate` 同一个值**（省略即上面的默认 115200）：这段等待读的就是固件控制台，
  速率必须和随后授权槽位用的一致，否则横幅和 `tuya>` 都会读成乱码、白等满上限。
  两段同源由代码钉住：等待用 `spec.baud_rate`，授权会话用 `job.baud_rate`（同一个字段），
  各有一条测试守着（bridge `an_authorization_lets_the_device_boot_before_the_slot_resets_it`、
  core `run_authorize_opens_the_session_at_the_jobs_baud_rate`）。
- 上限（core `WAIT_AFTER_FLASH_MAX`）取 30 s，与探测窗口 `boot_max_wait` 同量级；这是
  **保守估计不是实测值**（首启耗时上界从未测到）。
- 对 wire 契约**没有影响**：不新增帧、不新增 error_code，只是 `run_auth` 在设备沉默时
  最坏耗时多出这段等待。

### cancel

```json
{ "type": "cancel", "request_id": "j-001" }
```

- 置协作取消标志；任务以 `job_result ok=false error_code="cancelled"` 终结并释放串口。
- **作用域=发起任务的连接**（request_id 按连接命名空间隔离，跨连接不可取消他人任务）。
- 未命中（任务不存在/已结束）：静默忽略（log 记录，无应答帧）。
- 连接断开时该连接的在途任务自动 cancel（关标签页不会永久占串口）。
- **确认窗口内的 cancel 同样生效**（B8）：请求还在等用户确认时（尚未占串口）取消，
  Bridge 立刻结束等待、**绝不调用 backend**，回同一个 `error_code="cancelled"`
  （设备没被碰过，前端「已取消」的措辞是准确的），审计留 `decision=cancelled`。
  详见 §确认流程与 token 生命周期 的补充说明。

### check_port

```json
{ "type": "check_port", "port": "/dev/tty.xxx" }
```

```json
{ "type": "check_port_result", "port": "/dev/tty.xxx", "available": false, "reason": "occupied_by_bridge_job" }
```

```json
{ "type": "check_port_result", "port": "/dev/ttyACM0", "available": false,
  "reason": "occupied_by_other_process", "occupied_by": "ModemManager (812)" }
```

- bridge 自身持有 → `reason: "occupied_by_bridge_job"`（不做 OS 探测）。
- 否则 OS 级探测（tyutool-core `check_port_available`，实际尝试打开）：不可用 → `reason: "occupied_by_other_process"`；可用 → `available: true` 且 **省略 reason 字段**。
- `occupied_by`（可选）：**人类可读的占用者名字**，如 `"ModemManager (812)"`、`"picocom"`；
  多个占用者用 `, ` 连接。认不出来时**省略该字段**（绝不编造）。
  - 取名方式：Linux 用 `fuser` 拿 PID 再读 `/proc/<pid>/comm`；macOS 用 `lsof` 首列；Windows 拿不到。
  - 存在的理由：Ubuntu 上占住 `/dev/ttyACM*` 的多半是 **ModemManager**（对 CDC-ACM 设备自动
    发 AT 探测并 `TIOCEXCL` 独占），只回一个机器码时用户在界面上看到「被其他程序占用」
    却根本不知道该关谁。原始 OS 错误文本与 fuser/lsof 完整输出仍只进开发者日志。
  - 兼容：老 Web 端忽略该字段即可；老 bridge 不下发时前端回退通用文案。
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
| `bad_request` | port 缺失/空、data_bits·stop_bits·parity 取值非法、**整帧无法解码（如 `baud_rate` 传成字符串），见 §不可解析的请求帧** |
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
- run_auth：`{"step": "<snake_case>"}`——payload 恒为对象（前端按 request 种类无需特判非对象 payload）。
  **当前实际只会发一种：`{"step": "writing_auth"}`**，在授权写命令发给设备的那一刻发一帧。
  与 run_job 不同，run_auth 的 progress 是**收窄映射而不是透传**：bridge 走的是 core 的
  `FlashMode::Authorize`（单设备授权，见 §run_auth 的 core 入口），core 那侧发的是 `FlashEvent`，
  bridge 只把其中 `FlashMilestone::AuthWriteSent` 翻成 `writing_auth`，其余一律**不出帧**：
  - `AuthReadComplete` / `AuthConflict` **携带明文 uuid + authkey**，绝不上线路（bridge 侧拦截，
    不依赖前端的 SECURE_SILENT）；
  - `AuthReadEmpty` / `AuthWriteSkipped` 是结果而不是步骤，由终态 `job_result` 表达；
  - `JobSummary` / `Phase` / `Percent` / `Warning` / `Done` 与授权无关（`Done` 还带失败文案，
    bridge 自己有 `job_result`）。
  历史口径（B4~B18）是 `reading_mac` / `reading_auth` / `writing_auth` / `verifying` 四选一，
  来自 core 的 `BatchAuthStep`。那条路已废弃——**`reading_mac` 从此不会再出现**，
  前端保留对旧值的兼容分支无害，但不要再依赖它们出现。

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
| `port_busy` | 串口已被串口监视器会话 / 他人的烧后交接窗口持有（仲裁拒绝，立即返回） |
| `execution_busy` | 已有危险操作在途（含正在等用户确认），全局单一执行权拒绝，立即返回、不排队（B7，见 §单一执行权） |
| `cancelled` | cancel 帧 / 连接断开 / core 返回 FlashError::Cancelled；也包括「确认窗口内取消」（此时设备完全没被碰过） |
| `cancelled_after_write` | **仅 run_auth**：授权写命令**已经发给设备**之后才取消（bridge 以 core 的 `FlashMilestone::AuthWriteSent` 为分界，该里程碑在命令发出**之前**就发，宁可早不可晚）——授权码可能已经写进去了，见 §取消后的设备状态。message 只带**串口名**（这条路不读 MAC），**永不带 uuid** |
| `bad_request` | base64 解码失败 / mode 不支持 / **uuid·auth_key 为空缺失或长度不合法（uuid≠16且≠20 字符、auth_key≠32 字符，见 §run_auth）** / 同连接 request_id 重复在途 / **帧无法解码（含 run_job 缺 `baud_rate`、未知 `type`），见 §不可解析的请求帧**。共同点：**设备完全没被碰过**，前端文案不得暗示「可能已写入」 |
| `flash_failed` | run_job 执行层其余错误（message=FlashError 文本） |
| `device_no_response` | **仅 run_auth**：复位后整个探测窗口内设备**一个字节都没回**（core 判定，不是靠解析文案）。此时授权**一步都没做**，设备状态未被触碰。注意 bridge 在进授权槽位前已先做过一轮**被动自然启动等待**（不复位，见 §run_auth 的自然启动等待），所以拿到这个码时「首启慢」已经被等过了，文案口径是「再多等几秒没用，请断电重上电后重试」，**不要**说成「稍等几秒重试」，也**不要**说成「授权失败/状态未知」 |
| `auth_failed` | run_auth 执行层其余错误 |
| `user_rejected` | 危险操作的人工确认被拒绝 / 超时未答 / 确认通道被丢弃（B7） |
| `internal` | 执行线程 join 失败（panic 等）；或无系统熵可用、无法签发授权令牌 |
| `unsupported` | backend 不支持该操作（正常生产路径不出现） |

## 安全模型（B7，本地传输加固）

Bridge 是常驻在用户机器上的本地 helper，它能烧录固件、能覆盖不可撤销的授权码。
本节是这套权限的**完整口径**，前端按此实现，别自行加码或减码。

### 信任根：`Origin` 的真实边界

- `Origin` 是**浏览器被迫补上**的头，所以它挡得住「另一个网站的页面偷偷连本机 18730」这类跨站请求。
- 但**本机原生进程可以任意伪造** `Origin`（就是个 HTTP 头，没有任何东西强迫程序说真话），
  所以它是**过滤器，不是信任根**：白名单通过 ≠ 对面是 Cobuilder 网页。
- 真正的信任根是**用户那一次点击**：危险操作必须由本机弹出的确认对话框放行，
  token 只是把这一次点击**持久化**下来的收据，不是能力凭证。

### 分层权限：哪些操作需要人工确认

| 分层 | 帧 | 未授权连接 |
| --- | --- | --- |
| 只读 / 低危 | `hello`、`ports`、`check_port`、`cancel`、`serial_debug_*` | **照常可用**，不弹窗 |
| 危险（写设备） | `run_job`（烧录）、`run_auth`（写授权码） | **必须人工确认** |

- 低危一层刻意开放，保证「插线即就绪」：插上板子就能看到设备列表、就能开串口监视器看日志，
  不必先点一次授权。
- **`check_port` 明确不算危险操作**（它只探测串口能不能打开，不改设备任何状态）。这是契约，
  前端据此实现，不要改口径。
- `cancel` 同理不算危险操作：它只能取消**本连接自己**发起的任务（request_id 按连接命名空间隔离）。

### 确认流程与 token 生命周期

1. **弹确认**：未授权连接发来 `run_job` / `run_auth` → Bridge 在**本机**弹出系统确认框，
   内容含来源 Origin、操作类型、芯片、串口、固件大小；确认框**不含任何凭证**
   （`uuid` / `auth_key` 根本不进入确认请求）。默认按钮是「拒绝」。
   - ⚠ **确认框文案在交给会渲染富文本的对话框程序前一定先转义**（`&` → `&amp;`、`<` → `&lt;`、
     `>` → `&gt;`，只转一次）。原因：`chip_id` / `port` 是客户端可控字符串，而 Linux 下
     zenity 渲染 Pango markup、kdialog 会被 Qt 自动识别成富文本——不转义的话本机恶意进程
     能用 markup 改造这个正在问它自己的对话框（藏掉「来源」行、伪造官方横幅、把警告缩到看不见），
     那等于打穿整套确认设计赖以成立的唯一前提：**对话框必须如实描述它正在放行的那次操作**。
     恶意字符是**转义显示、不是丢弃**：被攻击的用户应当原样看到 `<b>` 这种字面量，
     而不是一段被悄悄改写过的文案。macOS（osascript）/ Windows（MessageBox）按纯文本渲染，不受影响。
2. **签发**：用户点「允许」→ 生成 token（32 字节 CSPRNG → base64url 无填充，43 字符），
   经 `auth_granted` 帧下发，再进入正常的 `progress` / `job_result` 流程。
   **刻意不塞进 `hello`**：`hello` 是连接即推，那时还没有任何用户点击可言。
3. **复用**：Web 端保存该 token，下次以 `ws://127.0.0.1:18730/?token=<token>` 重连，
   命中即**免弹窗**（也**不再下发** `auth_granted`——收据没变，无需重发）。
   - 命中判定**不是握手时定死一次**：每次危险操作都拿该连接握手时带的 token 去**重查授权存储**，
     所以 token 一旦被吊销，**已经开着的连接下一次危险操作就会重新弹确认**（见第 6 条）。
   - 参数名固定 `token`，重复 `token=` 取第一个，空值视为未带。
   - 值走 **percent-encoding**：`%XX` 会被解码（十六进制大小写不敏感）。
     **`+` 是字面量**（这是 URI query value，不是表单 body，不会被读成空格）。
     转义残缺（`%`、`%A`、`%ZZ`）或解出来不是合法 UTF-8 → 该 token 视为未带。
   - **校验失败只降级、绝不拒连**：未知 / 已吊销 / 换了 Origin / 不带 / 格式异常，
     一律照常 accept（`hello` + `ports` + `serial_debug_*` 都可用），只是下次危险操作再问一次用户。
4. **绑定 Origin**：查询键是 (token, Origin) 二元组，签发给 `http://localhost:3000` 的 token
   在 `http://127.0.0.1:3000` 上不生效，会退回弹确认。
5. **落盘**：`{config_dir}/tyutool-bridge/grants.json`（JSON，`{"version":1,"grants":[…]}`），
   unix 下权限 **0600**、经同目录临时文件 + rename 原子替换；进程重启后授权仍然有效。
   文件读不出或解析失败**不致命**：按「无授权」启动并告警，下一次授权覆盖它。
   ⚠ 该文件**含凭证**，提 issue / 发日志时**不要附带**。
6. **永不自动过期**：刻意不设有效期（时钟到点重新盘问用户，不解决任何问题）。
   回收手段只有**吊销**：托盘「撤销所有授权」→ 清空内存与文件、并把所有在线连接打回未授权
   （见 §auth_revoked）。托盘文案跟随系统语言（`zh*` → 中文，其余 → 英文，启动时定一次），
   本文档一律用中文那一档指代菜单项；英文系统上同一项显示为 “Revoke all authorizations”。**吊销对已经开着的连接立即生效**，不必等它重连：清空存储本身就让
   token 授权失效（每次危险操作重查存储），遍历在线连接则负责清掉「本次会话里点过允许」这半边、
   并推 `auth_revoked`。

**拒绝与超时共用同一个错误码**：用户点「拒绝」、**超时未答（默认 60s）**、确认通道被丢弃，
三者一律回 `job_result` `ok=false` `error_code="user_rejected"`，且**不推 `auth_granted`**、
串口不被占用、backend 完全不被调用。
→ **前端不要自己做本地超时兜底**：等待时长由 Bridge 收口，一定会等到一帧 `job_result`。这是对前端的承诺。

**确认窗口内的 cancel / 断连立刻收摊（B8）**：等确认的这段时间（默认最长 60s）里，
- 收到 `{"type":"cancel","request_id":…}` → **立刻**结束等待，回
  `job_result ok=false error_code="cancelled"`，backend 一次都不被调用，审计 `decision=cancelled`。
  修的是这个真 bug：以前该帧被丢弃（任务还没占串口，取消表里查不到它），
  页面已经显示「已取消」，用户几秒后又点了「允许」，设备照样被写。
- 连接断开（关标签页）→ 同样的收摊，并且**立刻释放单一执行权**；
  否则一个被遗弃的标签页会把整台 helper 锁死整个确认窗口（生产 60s）。
- ⚠ **已知取舍**：结束的是「等待」，不是「弹窗」。当前 `AuthPrompt` 契约没有关闭操作，
  已经弹在屏幕上的系统确认框**不会被程序收掉**，它会一直挂到用户自己点掉或平台放弃；
  那个迟到的答案**被忽略**（与 Windows MessageBox 那条已记录的取舍同源）。
  关键保证是：迟到的「允许」**再也无法启动操作**。前端不必也不能依赖弹窗消失来判断已取消，
  以 `job_result` 为准。
- ⚠ **已知取舍（二）：确认通过与占串口之间有一个纳秒级窗口**。用户点「允许」之后，
  实现先把这条待确认登记**摘掉**，再由 `arbiter.claim` 把任务**登记**进取消表；
  正好落在这两步之间的 `cancel` 帧两张表都查不到，会被丢弃——任务照常跑完并回 `ok=true`。
  **刻意不在代码里关掉它**：能确定性关掉它的两种做法都更糟——要么在拿到同意之前就先占串口
  （B7 明确禁止：被拒绝的操作绝不能占过串口），要么让 `cancel` 去「预约」一个**将来的**
  request id（会破坏前端用同一个 `request_id` 重试这个常见姿势）；而给一个只有几条指令宽的
  窗口写确定性测试，需要在 claim 路径里开测试缝，比这个 bug 本身更具侵入性。
  **缓解规则：终态 `job_result` 帧才是权威。** 已经显示「已取消」的前端，如果随后收到一帧
  说结果不是取消的终态帧，必须以后者为准并纠正界面（与 §取消后的设备状态 同一条规则）。

### 无人值守模式（`--headless`）

`--headless` 跑在 CI runner / 服务器 / ssh 会话里，**屏幕前没有人**。所以它不弹窗：

- **默认拒绝所有危险操作**（`run_job` / `run_auth` 一律 `user_rejected`），只读一层（`hello` /
  `ports` / `check_port` / `serial_debug_*`）照常可用。
  理由：没有用户可确认，而弹一个没人看得见的窗只会白等满 60s 确认窗口然后照样拒绝。
  每次拒绝会写一条 warn 日志，点名下面这个开关，避免运维只看到「烧录莫名失败」。
- **落盘的授权在这个模式下一概不算**（`GrantPolicy::Ignore`）：`grants.json` 是和托盘模式
  共用一份的，一条 grant 记录的是「**当时有人在键盘前点了一次允许**」，这份同意不能无声地
  延续到一个屏幕前没有人的会话里。所以哪怕本机存着一条对这个 Origin 完全有效的 token，
  `--headless`（未加下面那个开关）**照样拒绝**，`connect` 审计行也如实写 `pre_authorized=false`。
  理由很直白：既然专门设了 `--allow-unattended-writes` 这个开关，就等于宣布「无人值守必须显式声明」；
  一条历史 grant 不能替运维做这个声明。会话内真的弹窗点过允许那半边不受此影响——只是这个模式下
  永远不会有，因为它压根不弹。
- **`--allow-unattended-writes` 是唯一的显式开关**：加上它，危险操作**一律自动放行**，
  每次放行都在 warn 级别打一条日志（origin / 操作 / 芯片 / 串口），审计流水同样留
  `confirm … decision=approved`。
  ⚠ **说白了：开了这个开关，B7 的「必须由用户点一次」保证就不成立了**——本机任何进程只要
  连上 18730 就能写板子，日志是唯一剩下的痕迹。只在这台机器的控制台本身已经可信、
  且确实需要无人值守烧录时才用。
- 刻意**只做命令行开关、不认环境变量**：环境变量太容易被 shell profile / CI 全局 env / 父进程
  无声继承到一个根本没打算开这个口子的进程里。
- 不带 `--headless`（托盘模式）时该开关**不生效**：有 GUI 会话就有人可问，照旧弹确认框。
- `--help` / `-h` 打印上述两个开关及其风险。无法识别的参数**直接报错退出（exit 2）**，
  不静默忽略——拼错的 `--allow-unattended-write` 不能看起来像生效了。

### Linux 确认框的已知残余风险

zenity 是首选（它能把焦点放在**拒绝**那颗按钮上：`--default-cancel`，回车碰不出授权）。
没装 zenity 时退到 kdialog `--yesno`，而它**无法指定默认按钮**，所以在这条兜底路径上
**按回车有可能等于同意**。缓解手段是在 kdialog 的文案里加一行提示（告诉用户回车可能等于同意、
不确定就用鼠标点「否」），这行提示**只在 kdialog 分支出现**——一直显示的警告用户会学会忽略。
两条路径都不传 zenity 的 `--no-markup`：这个 flag 在老版本上不存在，未知 flag 会让 zenity
非零退出、把每次危险操作变成硬拒绝；**转义才是保证**，flag 只是可有可无的双保险。

日志与审计**永不打印完整 token**（只打前 6 字符 + 长度），也**永不打印授权文件内容**；
`uuid` / `auth_key` 一律不入日志——包括出错时的 `message`（`cancelled_after_write` 只带串口名），
也包括 `{:?}`：`WireAuth` / `AuthJobSpec` / `ClientMessage` 的 `Debug` 都是手写的，
凭证渲染成 `<redacted len=N>`、固件渲染成 `<base64 len=N>`，这是编译期保证而非评审承诺。

### 取消后的设备状态（前端文案依据）

同一个「取消」在设备上可能意味着两件完全不同的事，前端必须按 `error_code` 分开说，不能一律「未写入」：

| error_code | 设备状态 | 文案口径 |
| --- | --- | --- |
| `user_rejected` | **完全没碰设备**（拒绝 / 超时 / 通道丢弃都在弹窗阶段） | 「未执行」 |
| `cancelled`（确认窗口内取消） | **完全没碰设备**（串口都没占） | 「已取消，设备未改动」 |
| `port_busy` / `execution_busy` / `bad_request` | **完全没碰设备**（都在飞行前被拒；含**凭证为空/缺失/长度不合法**，那道校验在占串口和弹确认框之前就跑了） | 「未执行」 |
| `cancelled`（烧录途中取消） | **可能留下半个镜像**：已写的扇区不会回滚，板子可能起不来 | 「已取消，固件可能不完整，建议重烧」 |
| `cancelled_after_write` | **授权码可能已经写进设备**（KV 可覆盖；**OTP 是永久的**） | 「已取消，但授权码可能已被消耗」——**禁止**说「未写入」 |
| `device_no_response` | **完全没碰设备**（复位后设备一直没应答，授权一步没走） | 「设备一直没有响应。首启慢已经等过了，再多等几秒也没用：请给板子断电重上电后重试；若仍无响应，确认烧进去的固件能正常启动、且这个串口是设备的调试口」 |
| `flash_failed` / `auth_failed`（操作途中失败） | 可能留下部分状态（取决于失败在哪一步） | 「失败，设备状态未知，建议重试并检查」 |

- **`cancelled_after_write` 是唯一表示「凭证可能已被消耗」的码。** 拿到它的调用方
  不能把这枚 uuid/auth_key 当作未使用的库存再发给别的设备（core 那侧对应
  `confirm_row` 而不是 `release`）。
- 分界线是「有没有真的开始动设备」：飞行前（仲裁 / 校验 / 确认）取消是干净的，
  backend 一被调用就不再干净。前端只能靠这张表判断，不要去解析 `message` 文本。

### auth_granted（Bridge → Web，推送）

```json
{ "type": "auth_granted", "token": "Ab3-xYz…" }
```

用户刚刚点了「允许」，这是那一次点击的收据。收到即应持久化，下次连接带回（见上）。
无 `request_id`：它是连接级状态变更，不是某个任务的应答。

### auth_revoked（Bridge → Web，推送）

```json
{ "type": "auth_revoked" }
```

- 用户在托盘点了「撤销所有授权」：Bridge 清空授权存储后，**立即广播给所有在线连接**。
- 无 `request_id`（同 `auth_granted`，连接级状态变更）。
- 收到即应：**清掉本地保存的 token**，并把该连接当作未授权处理（下次烧录会重新弹确认）。
- 存在的理由：让前端**立刻**知道自己手里那张收据已经作废，把它清掉，而不是靠「下次烧录被重新弹窗 /
  失败」才发现，白白浪费一次尝试。
  权限本身不依赖这一帧：清空存储就已经让 token 授权在**已经开着的连接**上立即失效
  （每次危险操作重查存储）；遍历在线连接额外清掉「本次会话里点过允许」这半边（那半边不落存储）。

### 单一执行权（危险操作全局互斥）

**全进程同一时刻只允许一个危险操作在途**（`run_job` / `run_auth`），且**等用户确认的时间也算在内**。
冲突请求立即回 `job_result ok=false error_code="execution_busy"`，**不排队**（与串口仲裁同一原则）。

- 与串口仲裁的区别：串口仲裁是「一个串口一个持有者」，**换个串口就能并发**；
  单一执行权是「整台 helper 一次一个危险操作」，**换串口也不行**——
  堵住的是「别人正在烧录时，第二个标签页顺手烧另一块板」这条爆炸半径。
- 顺带解决**确认弹窗堆叠**：执行权在弹窗**之前**获取，所以弹窗还挂着时再来的危险操作
  直接 `execution_busy`，不会给用户叠出第二个弹窗。
- 释放时机是**终态 `job_result` 之后**（不是确认通过之后）：拒绝 / 超时 / 无熵 /
  `bad_request` / `port_busy` / 执行线程 panic / 客户端断开，所有退出路径都释放（RAII 守卫兜底）。
- **刻意不做成「按连接粘住」**：规则是「一次一个，谁问谁得」，不是「第一条连接终身持有执行权」。
  后者会让最早打开的那个标签页把 helper 锁死，而产品要求多标签页可以同时连上来围观
  （只有一个在驱动）。故上一个作业一结束执行权立刻回到自由态，下一个请求者即可拿到。
- 推论：`run_job` / `run_auth` 之间不可能再互相撞出 `port_busy`（并发根本不成立），
  该码只会由**串口监视器会话**或**他人的烧后交接窗口**触发。

### 审计行（格式冻结）

审计通道（`AuditSink`，库默认写 log target `bridge::audit`）**一个事件恰好一行**，
空格分隔的 `key=value`，取值原样写，缺值写 `-`：

```
connect origin=<origin> pre_authorized=<true|false> token=<redacted|->
confirm op=<flash|authorize> origin=<origin> chip=<chip_id> port=<port> firmware_bytes=<n|-> decision=<approved|rejected|timeout|abandoned|preauthorized|execution_busy|cancelled>
grant origin=<origin> op=<flash|authorize> token=<redacted>
revoke_all grants=<n>
```

- **每个危险操作都留恰好一行 `confirm`**，无论有没有弹窗：
  `preauthorized` = 该连接已持有有效授权（免弹窗）、`execution_busy` = 被单一执行权拒掉（未弹窗）、
  `cancelled` = 确认还挂着时被 `cancel` 帧 / 连接断开收走（弹过窗，用户没答上）。
  少了这几个取值，一次会话里第二次及以后的烧录在审计里就完全不可见。
- `token=<redacted>` 只有前 6 字符 + 长度（`redact_token`），**完整 token 永不入审计/日志**。
- `uuid` / `auth_key` **永不入审计**：确认请求（`ConfirmRequest`）里根本没有这两个字段。
- `revoke_all grants=<n>` 由托盘「撤销所有授权」打，`<n>` 是被清掉的授权条数。

## 通用约定

### 不可解析的请求帧：必须回一条错误应答

**解析失败或未知 `type` 的帧一律要有应答，不再静默丢弃。** 旧行为（只打一条 warn 就丢）在 pre 环境
造成过真实故障：前端发了 `run_job` 却因缺 `baud_rate` 解析失败，Bridge 一声不吭，页面永远停在
「等待确认·请在系统托盘…0%」，而托盘确认框根本没弹过——**客户端必须能快速失败，而不是永久挂着**。

关联方式是**尽力而为**：整体解析失败后，Bridge 再按宽松结构把顶层的 `type` 与 `request_id`
两个字符串抠出来（它们是顶层裸字符串，通常能在载荷坏掉的情况下幸存）。据此分三种情况：

| 情况 | 应答 |
| --- | --- |
| `type` == `"serial_debug_open"` | `serial_debug_open_failed error_code:"bad_request"`（该帧族本就不带 `request_id`） |
| 能抠出非空字符串 `request_id`（**含未知 `type`**） | `job_result ok=false error_code:"bad_request"` |
| 两者都抠不出（未知 `type` 且无可用 `request_id`，或压根不是 JSON） | log 后**丢弃** |

- 未知 `type` 只要带了 `request_id` 也回 `job_result bad_request`：让跑在更新协议上的客户端立刻失败，
  而不是干等。
- 最后一种刻意不应答：**没有任何东西可以对上号**，而伪造一个 `request_id` 会去应答一个客户端
  从未发起的任务。规避方法很简单——**请求帧永远带上 `request_id`**。
- ⚠ **`message` 只描述结构，绝不回显原始帧内容。** 客户端帧里有 base64 固件、设备 `uuid`、
  `auth_key`，而 serde 的原生错误文本会把出错的值原样引回来
  （`invalid type: string "…"`），这段文本既上线路又进日志文件。因此实现只保留：
  字段名（来自 Bridge 自己的结构体定义，不是帧内容）、失败类别、行列位置。
  典型取值：`missing required field \`baud_rate\`` / `wrong type for a field at line 1 column 210` /
  `malformed JSON at line 1 column 3` / `unknown or unsupported frame \`type\``。
  与既有的「日志永不打印 `uuid` / `auth_key` / 完整 token」是同一条纪律。
- 该应答**不占串口、不取执行权、不触达 backend**，设备完全没被碰过（见 §取消后的设备状态 中
  `bad_request` 那一行）。
- 多连接：ports 推送广播到所有连接；progress/job_result/check_port_result/serial_debug_* 只回发起连接。
- 同串口执行权全局唯一（跨连接互斥，任务与会话共表）；危险操作另有**全局单一执行权**（换串口也互斥，见 §单一执行权）；request_id 命名空间按连接隔离（不同连接可用相同 request_id）。
- `cancel` 只作用于任务：会话不在 request 索引里，取消帧碰不到串口监视器。
- 帧序：单连接内 hello → ports(初始) 保证有序；任务帧因果有序（progress 先于 job_result；busy=true 的 ports 推送不早于该任务首个可观察帧，busy=false 不早于 job_result）。

## 与《071 技术方案》样例的差异点（联调对账用）

1. `run_job.job.mode` 样例为 `"write"` —— 实现接受 `"write"`（默认）并映射 core `FlashMode::Flash`；`erase`/`read` 未实现（后续切片）。
2. progress payload 样例 `{"phase","percent","log"}` 为示意 —— run_job 为 FlashEvent 原样序列化；run_auth 为 `{"step":...}` 收窄映射（见上）。
3. `check_port_result.reason` 样例只出现 `occupied_by_other_process` —— 实现另有 `occupied_by_bridge_job`（bridge 自持）；不存在的串口暂也归 `occupied_by_other_process`。
4. `cancel` 样例未说明作用域 —— 实现为连接内命名空间（跨连接不可 cancel）。
5. run_auth 样例无存储位字段 —— 实现默认 Kv + Overwrite，OTP 需扩展 wire 字段。
6. `serial_debug_*` 帧已实现（B5），但**取子集**：open / chunk_batch（批量，非逐条 chunk）/ close / disconnected 四类，均**不带 request_id**；`cfg` 为 snake_case + 数字位宽（非 core 的 camelCase 拼写枚举）；`serial_debug_send`、归档回放、关键字过滤器、device_reset 等 serve 侧能力未实现。另新增技术方案未描述的**烧后交接窗口**（见上节），其预留态刻意不体现在 `ports.busy`。
7. hello 后固定紧跟一帧全量 ports —— 技术方案「连接即推全量」的落地形态（顺序保证写死）。
8. **技术方案原文「本期不做鉴权，仅靠 Origin 白名单」—— 实现没有照做**（见 [§安全模型](#安全模型b7本地传输加固)）。
   理由：`Origin` 只能挡跨站网页，**本机原生进程可随手伪造**，而 Bridge 能覆盖不可撤销的授权码，
   仅靠白名单等于「任何本机程序都能写你的板子」。故本期加固为：危险操作（`run_job` / `run_auth`）
   必须人工确认，那一次点击以 Origin 绑定的 token 落盘复用（`auth_granted` / 握手 `?token=`），
   吊销即失效并广播 `auth_revoked`；另加全局单一执行权与审计行。
   对前端的净增量：新增 `auth_granted` / `auth_revoked` 两帧、新增 `user_rejected` /
   `execution_busy` 两个 error_code、握手多一个可选 `?token=` 参数；
   低危帧（`hello` / `ports` / `check_port` / `cancel` / `serial_debug_*`）口径不变。
9. `cancel` 语义比样例多覆盖一段：**确认窗口内也能取消**（B8，见 §cancel）；
   run_auth 另新增 `cancelled_after_write` 一个 error_code，用来把「干净取消」和
   「写命令已发出、授权码可能已消耗」在结构上分开（前端文案依据见 §取消后的设备状态）。
10. **run_auth 不做「出厂默认 MAC」保护——这是明确裁决，不是遗漏。** GUI 的批量流水线
    (`run_batch_auth_slot`) 会先读 MAC，若读到出厂默认 MAC 就拒绝授权（防止把授权码浪费在
    未校准的板子上）。CoBuilder 单设备路径**不补**这道保护：
    ① 本路径的参考实现 `tyutool-cli authorize`（同一个 `run_authorize`）本身就没有这道检查，
    两者同构；
    ② 补它的前提是**读 MAC**，而现场固件恰恰不支持——2026-07-31 真机：shell 625 ms 就绪、
    固件探测为 Old，`read_mac` 三次重试全失败。把它补回去等于把刚拆掉的硬阻塞重新装上
    （那正是 `Failed to read MAC address` 的成因，见 §run_auth 的 core 入口）；
    ③ 它防的是「给未校准板子浪费授权码」，而 CoBuilder 的授权码是后端按 pid 现场分配的，
    浪费面比批量产线小得多。
    **若将来出现产线场景需要这道保护，正确做法是走 batch 路径，而不是给单设备路径加读 MAC。**
