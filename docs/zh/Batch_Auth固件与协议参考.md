# Batch Auth 固件与协议参考

本文面向**固件与工具链开发人员**，说明 Batch Auth 与设备侧 TAL CLI、KV 及 tyuTool 内部实现要点。**终端用户**请使用 [Batch Auth 批量授权使用指南](./Batch_Auth使用指南.md)。

- **英文版**：[Batch-Auth-Firmware-and-Protocol-Reference.md](../en/Batch-Auth-Firmware-and-Protocol-Reference.md)。

---

## 固件与协议要求

### 串口 CLI：auth、auth-read、read_mac

Batch Auth 通过**串口**与设备上的 **TAL CLI** 交互，**依赖固件侧已注册且可用的三条命令**：**`auth`**、**`auth-read`**、**`read_mac`**。名称与交互语义须与 tyuTool 内置协议一致；若未启用或未实现等效命令，**开始授权**与 **Read MAC** 无法完成。

在 **TuyaOpen** 公开仓库中，可按以下路径对照集成（路径相对于该仓库根目录）：

- **`apps/tuya_cloud/switch_demo/src/tuya_main.c`**：在 `user_main` 中，`tal_kv_init` 之后依次调用 **`tal_cli_init`**、**`tuya_authorize_init`**、**`tuya_app_cli_init`**，用于启动 CLI、注册授权子命令以及应用层命令表。若目标为 Linux 桌面等非嵌入式环境，部分初始化可能被条件编译跳过，以实际工程为准。
- **`src/tuya_cloud_service/authorize/tuya_authorize.c`**：**`tuya_authorize_init`** 向 CLI 注册 **`auth`**（按固定长度将 UUID、AuthKey 写入 KV）、**`auth-read`**（从 KV 读出当前授权）、**`auth-reset`**（按需清除 KV 授权项）等；KV 键名例如 **`UUID_TUYAOPEN`**、**`AUTHKEY_TUYAOPEN`**。Batch Auth 主要依赖 **`auth`** 与 **`auth-read`** 与 Excel/设备状态对齐。
- **`apps/tuya_cloud/switch_demo/src/cli_cmd.c`**：**`tuya_app_cli_init`** 将 **`read_mac`** 等命令注册到 **`s_cli_cmd`**。参考实现里 **`read_mac`** 常与 Wi‑Fi 相关宏一起编译；若芯片或网络方案不同，**仍须提供与协议兼容的 `read_mac`**（例如从本机网络栈读取 MAC 并输出），保证 PC 端解析到的 MAC 格式符合预期。

集成时请将上述调用与命令注册纳入应用启动流程；仅烧录固件而未打开对应 CLI 与三条命令时，Batch Auth 仍会失败。

### 限时关闭 CLI（须自行在固件中实现）

若希望在一段时间后使上述 CLI 命令（或整条串口调试/授权通道）失效，**须在固件中自行实现**，例如：**上电计时或累计运行时间到达阈值后不再注册相关命令**，或**发布镜像中不调用 `tal_cli_init` / 授权模块初始化**。**tyuTool 不提供「到期自动关闭 CLI」能力**。

### KV 密钥与数据安全（防泄露）

若不希望模组 **KV 存储**（含授权相关信息）因使用公开、默认或与竞品相同的密钥而被他人按同样参数解密读取，应在**设备固件**中自行修改 **`tal_kv_init`** 配置：将 **`seed`**、**`key`** 替换为**仅你方掌握**的随机串，并妥善保管、切勿泄露。KV 数据通常依赖该 **`seed` / `key`** 参与派生或加解密；密钥一旦泄露，对应 KV 内敏感内容（含授权信息）将面临被还原的风险。

示例（仅示意字段结构，**须改为你方自行生成且保密的值**，不得长期沿用文档、示例或 SDK 公开默认值）：

```c
tal_kv_init(&(tal_kv_cfg_t){
    .seed = "vmlkasdh93dlvlcy",
    .key  = "dflfuap134ddlduq",
});
```

生产与发布流程中需防止密钥进入公开仓库、日志或外发文档。

---

## 工作原理（实现摘要）

本节说明工具如何串联界面、后台线程、串口协议与 Excel，便于理解现象与日志；不要求终端用户阅读源码。

### 整体与 Excel

- 启动授权时加载工作簿，解析表头中的 **UUID**、**AUTHKEY**（或 **key**）等列；若缺少 **STATUS / MAC / TIMESTAMP**，在需要写入磁盘时自动追加。
- 对同一文件使用**文件锁**（`.lock`），避免多实例同时改写；统计 **总数 / 已用 / 剩余** 未用行。
- 写入设备并校验成功后，将对应行标记为 **USED**，并写入 **MAC** 与**时间戳**，然后保存；首次保存前可能对原表做 **`.bak` 备份**（若尚不存在）。

### 串口与可选烧录

- **未选择固件**：打开串口 → 排空上电启动阶段串口数据以便进入可交互状态 → 读 MAC 与授权。
- **已选择固件（当前为 ESP32）**：先校验串口 → 按芯片调用既有**烧录流程**（握手、擦除、写入、校验、重启）→ 通过 **DTR/RTS** 复位 → 再按授权波特率打开串口，进入读 MAC、授权与校验。

### 单台设备授权逻辑（摘要）

流程默认**每台物理设备有唯一 MAC**；若多台模组 MAC 相同，按 MAC 匹配与写入的逻辑可能无法区分真实设备。

1. **读 MAC**（失败会按上限重试）。
2. **读设备当前授权**：若设备为占位或未授权，则按表分配新行；若已授权，则与 Excel 中**按 MAC 匹配的行**比对。
3. **决策**：若与表中记录一致可跳过写入；若不一致或 MAC 不在表中，可能通过对话框询问是否**覆盖**（用户可 **Copy** 提示内容）。
4. **写入与校验**：从表中取下一条未使用的 UUID/AUTHKEY（或复用某行），经协议**写入**设备，再**回读比对**；全部通过后才**回写 Excel** 标记已用。

### 界面步骤条

界面步骤（打开串口、烧录、复位、读 MAC、写入授权、校验、关闭串口等）与上述阶段对应，用于展示进度与失败位置。
