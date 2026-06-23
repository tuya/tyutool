# 默认授权固件（auth-firmware）

批量烧录授权（batch-flash-auth）「默认授权固件」列表的源固件目录。维护者在此放置
固件 bin，手动触发 `.github/workflows/release-auth-firmware.yml` 即可自动生成
`auth-firmware.json` 并发布到 GitHub + Gitee 的 `auth-firmware` release。

## 目录结构与命名规则

```
assets/auth-firmware/
  <chip>/
    auth-firmware-<chip>-<version>.bin     # 固件
    auth-firmware-<chip>-<version>.txt      # 可选：版本说明（notes，纯文本）
```

示例：

```
assets/auth-firmware/
  esp32/
    auth-firmware-esp32-v1.0.0.bin
    auth-firmware-esp32-v1.1.0.bin
    auth-firmware-esp32-v1.1.0.txt
  bk7231n/
    auth-firmware-bk7231n-v1.0.0.bin
```

规则（违反会导致发布 workflow 报错退出）：

1. 一级目录名 = `chip`（小写，权威来源）。
2. bin 文件名必须为 `auth-firmware-<chip>-<version>.bin`，其中 `<chip>` 必须等于所在目录名。
3. `version` = 剥掉 `auth-firmware-<chip>-` 前缀与 `.bin` 后缀后的剩余部分（如 `v1.1.0`）。
4. 同名 `auth-firmware-<chip>-<version>.txt` 存在则其 trim 后内容作为该版本的 notes；不存在则省略。
5. **不要**建 `other/` 目录——`other` 是 auth-only 芯片，走 `FlashMode::Authorize`，不需要默认固件。

## 发布

在 GitHub Actions 页面手动运行 **Release auth-firmware**（`workflow_dispatch`）。流程：

- 用 `scripts/generate-auth-firmware-manifest.ts` 扫描本目录，算 sha256/size、拼 url、生成 manifest。
- 上传到 GitHub + Gitee 的 `auth-firmware` release：**已存在的 bin 跳过（固件按版本不可变），
  `auth-firmware.json` 始终覆盖**。

固件一经发布即视为不可变；要发新版本就放新 `version` 的 bin，不要改已发布的 bin。
