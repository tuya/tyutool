# 发布流程

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

## 范围说明

- Beta：`workflow_dispatch` 仅构建产物自测，不创建 Release。
- Gitee：当前仅同步 `latest.json`。
- `latest.json` 的更新说明为中英双语同一文本块，不按应用语言切换。
