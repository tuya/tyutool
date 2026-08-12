---
name: tyutool-release
description: Use when preparing or executing a tyutool release — version bump, CHANGELOG authoring, tagging, CI verification, or troubleshooting a stuck release.
---

# tyutool Release

## Overview

tyutool releases are version-tagged commits on `refactor/v3`. Pushing a `v*.*.*` tag triggers the CI release pipeline, which builds CLI + GUI artifacts for 5 platforms, then auto-publishes a GitHub Release after asset validation.

Two modes:

| Mode | When to use |
|---|---|
| **Automated** (`pnpm run release`) | First attempt; handles CHANGELOG polish interactively |
| **Manual** (`version:set` + git) | CHANGELOG already written; CI not available; staged approach |

---

## Mode A — Automated (recommended)

```bash
# 1. Ensure clean, synced refactor/v3 with CI passing
git checkout refactor/v3
git pull origin refactor/v3

# 2. Run the release script
pnpm run release 3.1.4
```

The script:
1. **Preflight** — validates branch, clean tree, no ahead/behind commits, tag doesn't exist, HEAD CI passed
2. **CHANGELOG draft** — inserts a new section; opens `$EDITOR` for polish
3. **CHANGELOG validation** — loops until draft marker removed and body is non-empty
4. **Bump** — updates all 5 version files + `Cargo.lock`
5. **Commit + tag + push** — `chore(release): v3.1.4`, pushes branch and tag

> GUI editor users: set `EDITOR="code --wait"` (or `"cursor --wait"`) so the script waits for you to save and close.

---

## Mode B — Manual (staged, via PR)

Use this when you've already written the CHANGELOG (e.g. during release prep), or when `gh` / push-to-`refactor/v3` permissions are unavailable.

Releases land on `refactor/v3` **via a PR** (never a direct push — AGENTS.md forbids direct commits to `refactor/v3`). The tag is created on the merged commit afterwards.

```bash
# 0. Start from a clean, synced refactor/v3
git checkout refactor/v3
git pull origin refactor/v3
git checkout -b yj/release-3.1.4        # <initials>/release-<version>

# 1. Bump versions + insert CHANGELOG draft
pnpm version:set 3.1.4

# 2. Edit CHANGELOG.md:
#    - Fill in 新功能 / Features and 问题修复 / Bug Fixes
#    - Delete the draft marker line: <!-- 润色后删除本行 / remove this line after editing -->

# 3. Update Cargo.lock
cargo update --workspace

# 4. Commit
# -u stages every tracked file already modified above: the version files (whose
# list lives in scripts/lib/version-files.mjs, not here), Cargo.lock, CHANGELOG.md
git add -u
git commit -m "chore(release): bump version to 3.1.4 and update changelog"

# 5. Push the branch and open a PR → refactor/v3
git push -u origin yj/release-3.1.4
gh pr create --base refactor/v3 --head yj/release-3.1.4 \
  --title "chore(release): v3.1.4" \
  --body "Release bump to 3.1.4. After merge, tag v3.1.4 on refactor/v3 to trigger release CI."

# 6. After the PR merges, sync local refactor/v3, then tag and push → triggers release CI
git checkout refactor/v3
git pull origin refactor/v3
git tag v3.1.4
git push origin v3.1.4
```

> The version bump should be the **only** change on the release branch — one commit, easy to review. If CI on the PR fails, fix on the release branch (or rebase) and re-push; do not tag until `refactor/v3` holds the merged commit.

---

## Files Updated by Version Bump

| File | Field |
|---|---|
| `package.json` | `"version"` |
| `src-tauri/tauri.conf.json` | `"version"` |
| `src-tauri/Cargo.toml` | `version = "..."` |
| `crates/tyutool-core/Cargo.toml` | `version = "..."` |
| `crates/tyutool-cli/Cargo.toml` | `version = "..."` |

---

## CHANGELOG Format

Each release entry uses a **two-block** bilingual layout: a full Chinese block, a `---` separator, then a full English block.

```markdown
## [3.2.2] - 2026-07-09

### 新功能

- `module`：中文描述

### 问题修复

- `module`：中文描述

---

### Features

- `module`: English description

### Bug Fixes

- `module`: English description
```

Rules:
- The `---` line separates the Chinese and English blocks within one version section — it is reserved for this purpose only.
- The ` / ` character sequence is **not** a language boundary; it may appear inside either language (e.g. `关闭 / 1 小时`, `off / 1h`).
- If a section has no English translation, omit the `---` and English block entirely; the Chinese block alone is fine.
- The in-app updater splits on `---` and shows only the block matching the user's language.

The `<!-- 润色后删除本行 / remove this line after editing -->` draft marker must be **removed** before the tag push — `pnpm run release` and `pnpm run release:gui` both enforce this.

---

## CI Pipeline (triggered by tag push)

```
tag v3.1.4 push
  └─ prepare        version strings, shared frontend dist
  └─ build-cli      5 platforms in parallel
  └─ build-gui      5 platforms in parallel (uses shared dist)
  └─ verify-release check assets + latest.json completeness
  └─ publish        un-draft GitHub Release (only if verify passes)
```

Expected artifacts: 5 CLI archives + 7 GUI installers + 4 portables + 1 macOS updater tarball + 4 updater signatures = 21 files.

---

## Preflight Checks (automated mode)

The script fails fast on any of:

- Not on `refactor/v3`
- Uncommitted changes in working tree
- Local branch ahead of or behind `origin/refactor/v3`
- Tag already exists locally or remotely
- HEAD CI run status ≠ `completed / success`

Fix each and re-run.

---

## Troubleshooting

| Symptom | Fix |
|---|---|
| `HEAD 的 CI 未通过` | Wait for CI or push a fix; re-run after it passes |
| `落后 origin/refactor/v3 N 个提交` | `git pull origin refactor/v3` |
| `本地已存在 tag vX.Y.Z` | `git tag -d vX.Y.Z` |
| `远端已存在 tag vX.Y.Z` | Version already released; pick a new version |
| Editor exits without changes | Set `EDITOR="code --wait"` |
| Release stays draft after CI | Check `verify-release.ts` output in CI logs; missing asset or `latest.json` mismatch |
| Need to retract a tag | `git tag -d vX.Y.Z && git push origin :refs/tags/vX.Y.Z`, then fix and re-tag |
