# Scenarios & Reference

## 1. Day-to-day feature development (personal branch)

Scenario: add support for a new chip in the firmware flash feature.

```bash
# Step 1: sync refactor/v3
git checkout refactor/v3 && git pull origin refactor/v3

# Step 2: create personal branch
git checkout -b ab/add-new-chip

# Step 3: layered commits
git add crates/tyutool-core/src/plugins/new_chip.rs
git commit -m "feat(firmware-flash): implement NewChip flash plugin"

git add crates/tyutool-core/src/plugins/mod.rs
git commit -m "feat(firmware-flash): register NewChip in plugin registry"

git add src/features/firmware-flash/chip-manifests.ts
git commit -m "feat(firmware-flash): add NewChip manifest with UI params"

# Step 4: rebase once a day during development
git fetch origin
git rebase origin/refactor/v3
# resolve conflicts if any, then: git rebase --continue

# Step 5: local validation
pnpm run lint && pnpm run build
cargo test -p tyutool-core

# Step 6: push and open MR: ab/add-new-chip → refactor/v3
git push -u origin ab/add-new-chip
```

---

## 2. Release (v3.x.x)

Scenario: current iteration is complete, ready to ship v3.1.0.

```bash
# Step 1: ensure refactor/v3 is latest and stable
git checkout refactor/v3 && git pull origin refactor/v3

# Step 2: full local validation
pnpm run lint && pnpm run build
cargo test -p tyutool-core && cargo test -p tyutool-cli

# Step 3: push tag — CI triggers build
git tag v3.1.0
git push origin v3.1.0

# CI automatically:
# ✓ Builds CLI binaries for 5 platforms (linux x86_64/aarch64, macOS x86_64/aarch64, Windows x86_64)
# ✓ Builds Tauri GUI (multi-platform)
# ✓ Creates GitHub Release with all artifacts attached
# ✓ Commits "chore: bump version to 3.1.0 [skip ci]" to refactor/v3

# Step 4: confirm all jobs pass in GitHub Actions
```

> **No manual version file edits needed.** To check the current version before tagging:
> ```bash
> grep '"version"' package.json
> # should match the last [skip ci] commit
> ```

---

## 3. v2 Hotfix

Scenario: after v2.3.2 ships, a crash is found when the serial port baud rate is zero.

```bash
# Step 1: create hotfix branch from master
git checkout master && git pull origin master
git checkout -b hotfix/uart-baud-crash

# Step 2: fix the bug
git add crates/tyutool-core/src/serial.rs
git commit -m "fix(serial): guard against zero baud rate in port open"

# Step 3: bump version manually (v2 has no CI write-back)
node scripts/bump-version.mjs 2.3.3
git add -u   # stage exactly what the script rewrote; the file list lives in
             # scripts/lib/version-files.mjs, not here
git commit -m "chore: bump version to 2.3.3"

# Step 4: validate
pnpm run lint && pnpm run build
cargo test -p tyutool-core

# Step 5: push and open MR → master
git push -u origin hotfix/uart-baud-crash

# Step 6: after master merge, push tag
git checkout master && git pull
git tag v2.3.3
git push origin v2.3.3
```

---

## 4. Conflict Resolution

### 4.1 Rebase (recommended)

```bash
git fetch origin
git rebase origin/refactor/v3

# On conflict:
# 1. git status — identify conflicting files
# 2. Edit files, remove conflict markers (<<<<<<< / ======= / >>>>>>>)
# 3. git add <resolved-files>
# 4. git rebase --continue
# To abort: git rebase --abort
```

### 4.2 Common file conflict strategies

| File | Strategy |
|---|---|
| `package.json` | Keep functional changes; `version` field follows the `[skip ci]` commit |
| `src-tauri/Cargo.toml` / `crates/*/Cargo.toml` | Keep `version` in sync with `package.json` |
| `src/features/firmware-flash/chip-manifests.ts` | Preserve both sides' new chip entries; check no duplicate `ChipId` |
| `crates/tyutool-core/src/plugins/mod.rs` | Preserve both sides' new plugin registration lines; check no duplicates |
| `src-tauri/src/lib.rs` | Preserve both sides' new Tauri commands; verify `invoke_handler` is complete |

### 4.3 Conflict prevention

- Rebase `refactor/v3` at least once a day during development
- Coordinate with teammates before touching `chip-manifests.ts` or `plugins/mod.rs` simultaneously
- Append new i18n keys at the end of their locale block to avoid mid-file conflicts

---

## 5. FAQs

### Accidentally committed the wrong file

```bash
# Not yet pushed — undo last commit (keep changes staged)
git reset --soft HEAD~1

# Already pushed — create a reverse commit
git revert HEAD
```

### Stash work to switch branches

```bash
git stash push -m "ab/my-feature: new chip flash plugin wip"
git checkout hotfix/urgent-fix
# after handling the hotfix...
git checkout ab/my-feature
git stash pop
```

### Accidentally deleted a branch

```bash
git reflog
# find the commit hash
git checkout -b ab/my-feature <commit-hash>
```

### CI build failed after tag was pushed

```bash
# fix the code on refactor/v3
git commit -m "fix: ..."
git push origin refactor/v3

# delete the old tag and re-push
git tag -d v3.1.0
git push origin --delete v3.1.0
git tag v3.1.0    # now points to the new commit
git push origin v3.1.0
```

### Version files out of sync

All version files must match. Check:
```bash
pnpm exec vitest run scripts/lib/version-files.test.ts
```
This asserts every file in `scripts/lib/version-files.mjs` carries the current
version, and that the list still covers every cargo workspace member.

Fix manually:
```bash
node scripts/bump-version.mjs <version>
```

### Commit only part of the changes in a file

```bash
git add -p src/features/firmware-flash/chip-manifests.ts
# y = stage this hunk, n = skip, s = split into smaller hunks
```

---

## 6. Branch Lifecycle Summary

```
<id>/*    ── create(refactor/v3) → develop → rebase → MR → merge into refactor/v3 → delete
hotfix/*  ── create(master) → fix → bump version → MR → merge into master → tag → delete
```

## 7. Release Artifacts

| Artifact | Build method | Platform |
|---|---|---|
| `tyutool-cli_linux_x86_64_<ver>.tar.gz` | musl static | Linux x86_64 |
| `tyutool-cli_linux_aarch64_<ver>.tar.gz` | musl static | Linux aarch64 |
| `tyutool-cli_macos_x86_64_<ver>.tar.gz` | Darwin | macOS Intel |
| `tyutool-cli_macos_aarch64_<ver>.tar.gz` | Darwin | macOS Apple Silicon |
| `tyutool-cli_windows_x86_64_<ver>.zip` | MSVC | Windows x86_64 |
| Tauri GUI installer | `pnpm run tauri:build` | Windows / macOS / Linux |
