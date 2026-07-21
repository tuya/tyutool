# 发布流程

## 临时低版本 GUI 调试包

当需要验证应用内更新展示时，可构建一个仅本次有效的低版本 GUI 包：

```bash
pnpm run build:gui:debug-version 0.0.1
```

- 该命令只临时覆盖 `src-tauri/Cargo.toml` 和 `src-tauri/tauri.conf.json` 的版本号
- 当前仅支持在 Windows 上执行；若在其他平台运行会直接报错退出
- 前端显示版本通过 `APP_VERSION` 注入为指定值
- 构建结束后会自动恢复 `src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json` 和根目录 `Cargo.lock`
- 为避免生成正式更新元数据，调试构建会临时关闭 `tauri.conf.json` 中的 updater artifact 输出
- 调试产物输出到 `.tmp/debug-builds/<version>-<timestamp>/`，当前复制 Windows 安装包产物（setup `.exe` 和 `.msi`）
- 该命令只用于本地调试，不会修改 `CHANGELOG.md` 或执行正式发布流程

## 前置一次性准备

```bash
cargo install git-cliff      # 生成 changelog 草稿
gh auth status               # 需登录 gh CLI（发布命令用它查 CI）
```

## 发布一个版本

1. 确认在 `refactor/v3`，工作区干净，已 pull，且该提交的 CI 已绿。
2. 运行：

   ```bash
   pnpm run release 3.0.14
   ```

3. 命令会打开编辑器显示 `CHANGELOG.md` 草稿（git-cliff 自动生成的英文要点）。
   - 删除 `<!-- 润色后删除本行 ... -->` 标记。
   - 把 `### 中文` / `### English` 两节润色成用户能看懂的要点。
   - 保存退出。
4. 命令随后自动 bump 版本、刷新 `Cargo.lock`、提交 `chore(release): v3.0.14`、打 tag、推送。
5. CI 构建全平台产物 → 生成 `latest.json` → `Verify & Publish` 校验通过后**自动转正式发布**。

## 失败恢复（tag 已推但构建/校验失败）

校验失败时 Release 停在草稿态，用户收不到更新。清理后重发：

```bash
gh release delete v3.0.14 --cleanup-tag --yes   # 删草稿 Release + 远端 tag
git tag -d v3.0.14                              # 删本地 tag
# 修复问题后重跑
pnpm run release 3.0.14
```

## 撤回已发布的版本

版本已正式发布（用户可见、`latest.json` 生效）后发现问题，按影响面处理：

```bash
# 1. 立即止血：把该 Release 改回草稿，其 latest.json 资源随之下线，
#    应用内更新器拿不到 → 停止向用户推送该版本
gh release edit v3.0.14 --draft=true

# 2. 如需彻底移除（含 tag）
gh release delete v3.0.14 --cleanup-tag --yes
git tag -d v3.0.14
```

- 若要让更新器回退到上一版本：把上一版的 `latest.json` 重新作为"最新"发布
  （在上一版 Release 上 `gh release upload <prev-tag> latest.json --clobber`，或重发上一版），
  使更新器读到旧版本号、不再提示升级。
- 已被用户下载/安装的安装包无法收回；撤回只能阻止尚未升级的用户继续装到问题版本。

## 范围说明

- Beta：`workflow_dispatch` 仅构建产物自测，不创建 Release。
- Gitee：当前仅同步 `latest.json`，且推送的是与 GitHub Release 完全相同的一份文件（不再单独生成 Gitee 版本）。
- `latest.json` 每个条目含三个下载地址：`url`（GitHub）、`url_gitee`（Gitee，产物未同步前不可用）、`url_tuya`（Tuya OSS，由外部 tuyaopen-oss-publish 流水线上传产物）。客户端更新逻辑仅使用 `url`；镜像字段供外部系统读取。
- `latest.json` 的更新说明为中英双语同一文本块，不按应用语言切换。
