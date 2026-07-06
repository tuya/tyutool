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

## Mode B — Manual (staged)

Use this when you've already written the CHANGELOG (e.g. during release prep).

```bash
# 1. Bump versions + insert CHANGELOG draft
pnpm version:set 3.1.4

# 2. Edit CHANGELOG.md:
#    - Fill in 新功能 / Features and 问题修复 / Bug Fixes
#    - Delete the draft marker line: <!-- 润色后删除本行 / remove this line after editing -->

# 3. Update Cargo.lock
cargo update --workspace

# 4. Commit
git add package.json src-tauri/tauri.conf.json src-tauri/Cargo.toml \
        crates/tyutool-core/Cargo.toml crates/tyutool-cli/Cargo.toml \
        Cargo.lock CHANGELOG.md
git commit -m "chore(release): bump version to 3.1.4 and update changelog"

# 5. Push and wait for CI to pass
git push origin refactor/v3

# 6. Tag and push → triggers release CI
git tag v3.1.4
git push origin v3.1.4
```

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

Each release entry uses bilingual inline bullets:

```markdown
## [3.1.4] - 2026-07-01

### 新功能 / Features

- `module`：中文描述 / English description

### 问题修复 / Bug Fixes

- `module`：中文描述 / English description

### 工程改进 / Engineering

- `ci`：中文描述 / English description
```

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
