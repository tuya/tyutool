# AI产品数据采集调试协议

> Status: `Draft`

| 日期       | 版本  | 修订 | 说明                          |
| ---------- | ----- | ---- | ----------------------------- |
| 2025-05-16 | Draft | 奥丁 | Initial Version               |
| 2025-06-04 | Draft | 奥丁 | 在协议头中添加session id      |
| 2025-06-09 | Draft | 奥丁 | 去除不支持的字段              |
| 2025-06-12 | V0.1  | 奥丁 | 修改协议头，与基座2.0保持一致 |

## 需求和背景

其它需求方：《AI产品全链路数据采集方案》

### 问题描述

- 涉及多模态数据交互(文本、音频、视频、图片、表情、行为等)
- 调试和测试阶段难以通过传统日志方式采集所有数据
- 需要实时监控设备与云端的交互数据流
- 缺乏统一的问题诊断和分析机制

### 目标

1. 实现AI产品全链路数据采集
2. 支持实时数据监控和分析
3. 提供问题快速定位和诊断能力
4. 适用于开发调试和测试验证阶段

## 参考文档

- [2.0 Essential Protocol](https://registry.code.tuya-inc.top/TuyaBEMiddleWare/steam/-/issues/1#packet "2.0 Essential Protocol")

## 方案

### 整体架构

设计一套专用的旁路调试协议:

- 在设备与云端之间建立旁路数据通道
- 通过"AI调试助手"工具接收和处理数据
- 不影响设备与云端的正常业务交互

### 核心特性

1. 支持多模态数据采集
2. 实时数据监听分析
3. 双向数据传输能力
4. 可选的数据加密压缩

### 部署方式

- 设备端集成调试协议SDK
- 云端部署数据采集服务
- 调试工具通过TCP 5055端口接入

## 协议

1. 默认使用TCP协议
2. 支持双向数据传输
3. 流ID为奇数时为设备端上行发送，偶数时为设备端下行接收
4. 默认security_level为0x0，不加密、无IV、含签名
5. 数据加密（V1不支持）
6. 数据压缩（V1不支持）
7. 字节序采用大端序（Big Endian）

### 协议头

| 字段名           | 字段长度(Bits)      | 说明                                                         |
| ---------------- | ------------------- | ------------------------------------------------------------ |
| `magic`          | 32                  | Magic字段，用于帧同步，现在为 `0x54594149`                                        |
| `direction`      | 2                   | 数据流方向，`0`为设备发送给云端，`1`为云端发送给设备，`2`为设备和测试终端交互的数据       |
| `reserve`        | 6                   | 保留位，现在为 `0x00`，用于将来扩展新功能时兼容旧版本                                        |
| `version`        | 8                   | 版本号，现在为 `0x01`                                        |
| `sequence`       | 16                  | 序列号，自增，不为 0                                         |
| `frag_flag`      | 2                   | 分片标志：<br>- `00` 不分片<br>- `01` 分片开始<br>- `10` 分片中<br>- `11` 分片结束 |
| `security_level` | 5                   | 安全等级（支持 L2、L3、L4，不支持 L1、L5）：<br>- `0x0` L0，表明不加密，不签名<br>- `0x1` L1<br>- `0x2` L2<br>- `0x3` L3<br>- `0x4` L4<br>- `0x5` L5 |
| `iv_flag`        | 1                   | 是否包含加密向量：<br>- `0` 不包含，使用前序 TransportPacket iv 进行解密 <br>- `1` 包含，使用当前 TransportPacket iv 进行解密 |
| `reserve`        | 8                   | 保留位，现在为 `0x00`，用于将来扩展新功能时兼容旧版本        |
| `iv`             | `16 * 8`            | 加密向量，根据 security_level 指定 iv 长度：<br>- L2：12 Bytes<br>- L3：16 Bytes<br>- L4：16 Bytes |
| `length`         | 32                  | Payload 长度 + Signature 长度（若有）                        |
| `payload`        | `(length - 32) * 8` | 根据 security_level 指定加密方式解密，解密后为 Packet        |
| `signature`      | `32 * 8`            | 根据 security_level 指定签名方式签名（显式，防篡改）         |

### 数据包格式

这是应用层 Packet，后文简称为 Packet。

| 字段名           | 字段长度(Bits) | 说明                                                         |
| ---------------- | -------------- | ------------------------------------------------------------ |
| `type`           | 7              | - `1` ~~ClientHello~~<br>- `2` ~~AuthenticateRequest~~<br>- `3` ~~AuthenticateResponse~~<br>- `4` Ping<br>- `5` Pong<br>- `6` ~~ConnectionClose~~<br>- `7` ~~SessionNew~~<br>- `8` ~~SessionClose~~<br>- `9` ~~ConnectionRefreshRequest~~<br>- `10` ~~ConnectionRefreshResponse~~<br>- `30` Video<br>- `31` Audio<br>- `32` Image<br>- `33` File<br>- `34` Text<br>- `35` Event |
| `attribute_flag` | 1              | 属性标识符：<br>- `0` 不包含属性<br>- `1` 包含属性           |
| `attributes`     |                | 根据 attribute_flag 决定是否包含 attributes                  |
| `length`         | 32             | Payload 长度（最大 4 GB）                                    |
| `payload`        | `length * 8`   | Payload 内容                                                 |

对于每种细分类型 Packet 承载的一次性内容或一段连续内容，比如一段连续的视频流、一段连续的音频流、一段连续的文件内容字节流，其第一个 Packet 应当携带对内容的描述信息，这些描述信息在后文称之为 Attributes，第一个 Packet 后的 Packets 复用第一个 Packet 携带的 Attributes，通过 Packet 的 `attribute_flag` 标识 Packet 是否包含 Attributes。

### 属性（Attributes）

| 字段名                      | 字段长度(Bits) | 说明                                                         |
| --------------------------- | -------------- | ------------------------------------------------------------ |
| `attributes_length`         | 32             | 所有属性的长度                                               |
| attribute[i].`type`         | 16             | 属性类型                                                     |
| attribute[i].`payload_type` | 8              | 属性内容类型<br> - `0x01` uint8<br> - `0x02` uint16<br> - `0x03` uint32<br> - `0x04` uint64<br> - `0x05` bytes<br> - `0x06` string |
| attribute[i].`length`       | 32             | 属性长度                                                     |
| attribute[i].`payload`      | `length * 8`   | 属性内容                                                     |

#### 属性类型

> 以下属性类型为AI基座2.0协议原生属性类型，本协议只需支持部分类型。

| Attribute Type                    | Attribute Description                                        | Attribute Payload                                            |
| --------------------------------- | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `11` ~~ClientType~~               | Client 类型                                                  | **uint8**<br><br>- `1` Device<br>- `2` APP                   |
| `12` ~~ClientID~~                 | 派生 Client ID                                               | **string**                                                   |
| `13` ~~EncryptRandom~~            | 用于生成加密密钥的随机数，复用连接时复用随机数               | **string**                                                   |
| `14` ~~SignRandom~~               | 用于生成签名密钥的随机数，复用连接时复用随机数               | **string**                                                   |
| `15` ~~MaxFragmentLength~~        | Client 能处理的 Transport Payload 最大长度                   | **uint32**                                                   |
| `16` ~~ReadBufferSize~~           | Server Socket 接收缓冲区大小                                 | **uint32**                                                   |
| `17` ~~WriteBufferSize~~          | Server Socket 发送缓冲区大小                                 | **uint32**                                                   |
| `18` ~~DerivedAlgorithm~~         | 派生 Client ID 的算法                                        | **string**                                                   |
| `19` ~~DerivedIV~~                | 派生 Client ID 的 IV                                         | **string**                                                   |
| `21` ~~Username~~                 | 用户名                                                       | **string**                                                   |
| `22` ~~Password~~                 | 密码                                                         | **string**                                                   |
| `23` ~~ConnectionID~~             | 连接 ID                                                      | **string**<br><br>Version 4 UUID                             |
| `24` ~~ConnectionStatusCode~~     | 连接状态码                                                   | **uint16**<br><br>- `200` OK<br>- `400` Bad Request<br>- `401` Unauthenticated<br>- `404` Not Found<br>- `408` Request Timeout<br>- `500` Internal Server Error<br>- `504` Gateway Timeout<br>- `601` CloseByClient<br>- `602` CloseByReuse<br>- `603` CloseByIO<br>- `604` CloseByKeepalive<br>- `605` CloseByExpire |
| `25` LatestExpireTimestamp        | 最新的 Connection 过期时间戳，秒                             | **uint64**                                                   |
| `31` ~~ConnectionCloseErrorCode~~ | 连接关闭错误码                                               | **uint16**<br><br>- `200` OK<br>- `400` Bad Request<br>- `401` Unauthenticated<br>- `404` Not Found<br>- `408` Request Timeout<br>- `500` Internal Server Error<br>- `504` Gateway Timeout<br>- `601` CloseByClient<br>- `602` CloseByReuse<br>- `603` CloseByIO<br>- `604` CloseByKeepalive<br>- `605` CloseByExpire |
| `41` ~~BizCode~~                  | 业务 Code                                                    | **uint32**<br><br>- `100` BizTypeLLMChart_100                |
| `42` ~~BizTag~~                   | 业务 Tag                                                     | **uint64**                                                   |
| `43` ~~SessionID~~                | 会话 ID                                                      | **string**<br><br>Version 4 UUID                             |
| `44` ~~SessionStatusCode~~        | 会话状态码                                                   | **uint16**<br><br>- `200` OK<br>- `400` Bad Request          |
| `45` ~~AgentToken~~               | Agent Token                                                  | **string**                                                   |
| `51` ~~SessionCloseErrorCode~~    | 会话关闭错误码                                               | **uint16**<br><br>- `200` OK<br>- `400` Bad Request<br>- `401` Unauthenticated<br>- `404` Not Found<br>- `408` Request Timeout<br>- `500` Internal Server Error<br>- `504` Gateway Timeout<br>- `601` CloseByClient<br>- `602` CloseByReuse<br>- `603` CloseByIO<br>- `604` CloseByKeepalive<br>- `605` CloseByExpire |
| `61` EventID                      | 事件 ID                                                      | **string**<br><br>Version 4 UUID                             |
| `62` EventTimestamp               | 事件时间戳，透传给业务层                                     | **uint64**                                                   |
| `63` StreamStartTimestamp         | 媒体流开始时间戳                                             | **uint64**                                                   |
| `71` VideoCodecType               | 视频编码类型                                                 | **uint16**<br><br>最大值为 2^16 -1<br>- `0` VideoCodecMPEG4<br>- `1` VideoCodecH263<br>- `2` VideoCodecH264<br>- `3` VideoCodecMJPEG<br>- `4` VideoCodecH265<br>- `5` VideoCodecYUV420<br>- `6` VideoCodecYUV422<br>- `99` VideoCodecMax |
| `72` VideoSampleRate              | 视频采样率                                                   | **uint32**<br><br>最大值为 2^32 - 1<br>- `90000`             |
| `73` VideoWidth                   | 视频宽                                                       | **uint16**<br><br>最大值为 2^16 - 1 = 65535                  |
| `74` VideoHeight                  | 视频高                                                       | **uint16**<br><br>最大值为 2^16 - 1 = 65535                  |
| `75` VideoFPS                     | 视频帧率                                                     | **uint16**<br><br>最大值为 2^16 -1 = 65535                   |
| `81` AudioCodecType               | 音频编码类型                                                 | **uint16**<br><br>最大值为 2^16 - 1<br>- `100` AudioCodecADPCM<br>- `101` AudioCodecPCM<br>- `102` AudioCodecAACRaw<br>- `103` AudioCodecAACADTS<br>- `104` AudioCodeAACLATM<br>- `105` AudioCodecG711U<br>- `106` AudioCodecG711A<br>- `107` AudioCodecG726<br>- `108` AudioCodecSPEEX<br>- `109` AudioCodecMP3<br>- `110` AudioCodecG722<br>- `111` AudioCodecOpus<br>- `199` AudioCodecMax<br>- `200` AudioCodecInvalid |
| `82` AudioSampleRate              | 音频采样率，单位 Hz                                          | **uint32**<br><br>最大值为 2^32 - 1                          |
| `83` AudioChannels                | 音频通道数                                                   | **uint16**<br><br>- `0` AudioChannelMono<br>- `1` AudioChannelStero |
| `84` AudioBitDepth                | 音频样本位深                                                 | **uint16**<br><br>最大值为 2^16 - 1                          |
| `91` ImageFormat                  | 图像格式                                                     | **uint8**<br><br>- `1` JPEG<br>- `2` PNG                     |
| `92` ImageWidth                   | 图像宽度                                                     | **uint16**<br><br>最大值为 2^16 - 1 = 65535                  |
| `93` ImageHeight                  | 图像高度                                                     | **uint16**<br><br>最大值为 2^16 - 1 = 65535                  |
| `101` FileFormat                  | 文件格式                                                     | **uint8**<br><br>最大值为 2^8 - 1 = 255<br>- `1` MP4<br>- `2` OGG-OPUS<br>- `3` PDF<br>- `4` JSON<br>- `5` 监控日志<br>- `6` 扫地机地图 |
| `102` FileName                    | 文件名                                                       | **string**<br><br>最大长度为 2^8 - 1 = 255                   |
| `111` UserData                    | 嵌套的完整 Attributes，由每个业务自行定义其中的 Attribute Type，必须大于或等于 128，容量为 128 | **bytes**                                                    |
| `112` SessionIDList               | SessionID 列表，包含多个 SessionID 时分隔符为 `,`            | **string**                                                   |
| `113` ClientTimestamp             | Client 时间戳，毫秒                                          | **uint64**                                                   |
| `114` ServerTimestamp             | Server 时间戳，毫秒                                          | **uint64**                                                   |

### 视频（Video）

| Field         | Bits         | Description                                                  |
| ------------- | ------------ | ------------------------------------------------------------ |
| `id`          | 16           | 数据 ID                                                      |
| `stream_flag` | 2            | 数据流标志位：<br>- `00` 数据流只有一个 Packet<br>- `01` 数据流开始<br>- `10` 数据流中<br>- `11` 数据流结束 |
| `reserve`     | 6            | 保留位，为 `000000`                                          |
| `timestamp`   | 64           | 发送时间戳，毫秒                                             |
| `pts`         | 64           | 渲染时间戳，微秒                                             |
| `length`      | 32           | Payload 长度                                                 |
| `payload`     | `length * 8` | Payload 内容                                                 |

- Required attributes for first Video Packet
  - VideoCodecType
  - VideoSampleRate
  - VideoWidth
  - VideoHeight
  - VideoFPS
- Optional attributes
  - UserData
  - SessionIDList

### 音频（Audio）

| Field         | Bits         | Description                                                  |
| ------------- | ------------ | ------------------------------------------------------------ |
| `id`          | 16           | 数据 ID                                                      |
| `stream_flag` | 2            | 数据流标志位：<br>- `00` 数据流只有一个 Packet<br>- `01` 数据流开始<br>- `10` 数据流中<br>- `11` 数据流结束 |
| `reserve`     | 6            | 保留位，为 `000000`                                          |
| `timestamp`   | 64           | 发送时间戳，毫秒                                             |
| `pts`         | 64           | 渲染时间戳，微秒                                             |
| `length`      | 32           | Payload 长度                                                 |
| `payload`     | `length * 8` | Payload 内容                                                 |

- Required attributes for first Audio Packet
  - AudioCodecType
  - AudioSampleRate
  - AudioChannels
  - AudioBitDepth
- Optional attributes
  - UserData
  - SessionIDList

### 图片（Image）

| Field         | Bits         | Description                                                  |
| ------------- | ------------ | ------------------------------------------------------------ |
| `id`          | 16           | 数据 ID                                                      |
| `stream_flag` | 2            | 数据流标志位：<br>- `00` 数据流只有一个 Packet<br>- `01` 数据流开始<br>- `10` 数据流中<br>- `11` 数据流结束 |
| `reserve`     | 6            | 保留位，为 `000000`                                          |
| `timestamp`   | 64           | 时间戳                                                       |
| `length`      | 32           | Payload 长度                                                 |
| `payload`     | `length * 8` | Payload 内容                                                 |

- Required attributes
  - ImageFormat
- Optional attributes
  - ImageWidth
  - ImageHeight
  - UserData
  - SessionIDList

### 文件（File）

| Field         | Bits         | Description                                                  |
| ------------- | ------------ | ------------------------------------------------------------ |
| `id`          | 16           | 数据 ID                                                      |
| `stream_flag` | 2            | 数据流标志位：<br>- `00` 数据流只有一个 Packet<br>- `01` 数据流开始<br>- `10` 数据流中<br>- `11` 数据流结束 |
| `reserve`     | 6            | 保留位，为 `000000`                                          |
| `length`      | 32           | Payload 长度                                                 |
| `payload`     | `length * 8` | Payload 内容                                                 |

- Required attributes
  - FileFormat
  - FileName
- Optional attributes
  - UserData
  - SessionIDList

### 文本（Text）

| Field         | Bits         | Description                                                  |
| ------------- | ------------ | ------------------------------------------------------------ |
| `id`          | 16           | 数据 ID                                                      |
| `stream_flag` | 2            | 数据流标志位：<br>- `00` 数据流只有一个 Packet<br>- `01` 数据流开始<br>- `10` 数据流中<br>- `11` 数据流结束 |
| `reserve`     | 6            | 保留位，为 `000000`                                          |
| `length`      | 32           | Payload 长度                                                 |
| `payload`     | `length * 8` | Payload 内容                                                 |

- Optional attributes
  - SessionIDList

### 事件（Event）

> 命令下发和响应通过Event来实现

| Field     | Bits         | Description                                                  |
| --------- | ------------ | ------------------------------------------------------------ |
| `type`    | 16           | - `0` Start<br>- `1` PayloadsEnd<br>- `2` End<br>- `3` OneShot<br>- `4` ChatBreak<br>- `5` ServerVAD<br>- `6` AgentTokenExpired<br>- `0xf000` MonitorTypeFilter(UserData属性长度固定为8字节，每个bit代表对应的过滤类型，默认只支持Bit30~35) |
| `length`  | 16           | Payload 长度                                                 |
| `payload` | `length * 8` | Payload 内容                                                 |

- Required attributes
  - SessionID
  - EventID
- Optional attributes
  - UserData

### 业务流程

1. 连接建立
   - 服务端监听 TCP 5055 端口
   - 客户端连接服务端
2. 心跳维护（前期暂不支持）
   - 客户端定期发送 PING (建议间隔30秒)
   - 服务端回复 PONG
   - 90秒未收到 PONG 则断开重连

3. 订阅监听数据类型
   - 客户端根据业务场景注册需要监听的数据类型

4. 订阅数据格式说明
   - 采用`Event`类型下发MonitorTypeFilter
   - `type`为`0xf000`
   - `SessionID`为`AI调试助手`的SessionID，可随机生成
   - `EventID`为`AI调试助手`的EventID，可随机生成
   - `UserData属性`为`uint64_t`类型,Big endian序
     - `UserData属性`的`length`固定为`8`Bytes
     - 选中的类型对应的位为`1`，未选中的类型对应的位为`0`
     - `bitmap`类型定义如下：
       - Bit`30` == 1 Video
       - Bit`31` == 1 Audio
       - Bit`32` == 1 Image
       - Bit`33` == 1 File
       - Bit`34` == 1 Text
       - Bit`35` == 1 Event

5. 数据传输
   - AI多模态数据: 通过 Packet 传输
   - **TODO:** 设备日志数据: 通过 Packet 传输，Packet.type类型为`Text` 