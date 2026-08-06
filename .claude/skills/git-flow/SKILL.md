---
name: git-flow
description: Git branch model for the tyutool repository, covering branch creation, merging, releases, and hotfixes. Triggers on git-flow, branch management, release, feature, hotfix, and related keywords.
---

# Git-Flow · tyutool

tyutool is a firmware flash tool for Tuya-class IoT devices. Release artifacts: CLI binaries (5 platforms) and a desktop GUI (Tauri 2).

## Branch Model

```
  hotfix ─────────┐          ┌──── hotfix
                   ▼          ▼
  master ●────────●──────────●───→  ← v2.x stable (archived, hotfix only)

  refactor/v3 ●───●───●───●──────→  ← v3 main dev branch (currently active)
                  \   \
  <id>/*  ●───────┘   ●───→          ← personal branches (<initials>/<description>)
```

> v3.x.x tags are pushed directly onto `refactor/v3` commits. CI triggers the build and auto-writes the version bump back.

## Branch Reference

### Long-lived branches

| Branch | Role | Rules |
|---|---|---|
| `refactor/v3` | v3 dev integration branch (current trunk) | No direct push; personal branches merged via MR after local validation |
| `master` | v2.x stable (archived) | Accepts `hotfix/*` MRs only, maintains the v2 series |

### Short-lived branches

| Type | Format | Base | Target | Purpose |
|---|---|---|---|---|
| Feature | `<initials>/<description>` | `refactor/v3` | `refactor/v3` | Day-to-day feature work |
| Hotfix (v2) | `hotfix/<description>` | `master` | `master` | Urgent fixes for published v2 releases |

> `<initials>` is the developer's name initials, e.g. `ab`.
> Description: kebab-case English, e.g. `ab/serial-debug-scroll`, `ab/add-new-chip`.

### Branch decision tree

```
Is this an urgent bug in a published v2 release (master / v2.x tag)?
├─ Yes → hotfix/<description>  (based on master)
└─ No  → <initials>/<description>  (based on refactor/v3)
```

## Core Workflows

### 1. Day-to-day feature development

```bash
# Create personal branch from refactor/v3
git checkout refactor/v3 && git pull origin refactor/v3
git checkout -b ab/my-feature

# Develop & commit (follow commit convention)
git add src/features/firmware-flash/chip-manifests.ts
git commit -m "feat(firmware-flash): add new chip manifest"

# Sync trunk periodically to keep linear history
git fetch origin
git rebase origin/refactor/v3

# Push
git push -u origin ab/my-feature

# Validate locally (see release checklist) → open MR into refactor/v3
```

### 2. Release (v3.x.x)

```bash
# Step 1: ensure refactor/v3 is up-to-date and stable
git checkout refactor/v3 && git pull origin refactor/v3

# Step 2: local validation
pnpm run lint && pnpm run build
cargo test -p tyutool-core && cargo test -p tyutool-cli

# Step 3: push tag — CI triggers build + auto version bump
git tag v3.1.0
git push origin v3.1.0

# CI automatically:
# 1. Builds CLI binaries for 5 platforms + Tauri GUI
# 2. Creates GitHub Release with all artifacts
# 3. Commits version bump to refactor/v3  (chore: bump version to 3.1.0 [skip ci])
```

> Do not manually edit version files — `scripts/bump-version.mjs` updates all of them:
> `package.json`, `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`, `crates/*/Cargo.toml`

### 3. Beta build (manual trigger)

Trigger `workflow_dispatch` in GitHub Actions. CI will:
- Use the version in the current `package.json`; artifact filenames get a `.beta` suffix
- No tag pushed, no version write-back committed

### 4. v2 Hotfix

```bash
# Step 1: create hotfix branch from master
git checkout master && git pull origin master
git checkout -b hotfix/uart-baud-crash

# Step 2: fix & bump version manually (v2 has no CI write-back)
node scripts/bump-version.mjs 2.3.3
git add -u   # stage exactly what the script rewrote; the file list lives in
             # scripts/lib/version-files.mjs, not here
git commit -m "chore: bump version to 2.3.3"

git add crates/tyutool-core/src/serial.rs
git commit -m "fix(serial): guard against zero baud rate in port open"

# Step 3: push and open MR → master
git push -u origin hotfix/uart-baud-crash

# Step 4: after master MR merges, tag to trigger CI
git checkout master && git pull
git tag v2.3.3
git push origin v2.3.3
```

## Commit Convention

Format: `<type>(<scope>): <message>`, message ≤ 72 characters.

| type | meaning |
|---|---|
| `feat` | new feature |
| `fix` | bug fix |
| `chore` | build, deps, config (no functional change) |
| `docs` | documentation |
| `refactor` | refactor (no behavior change) |
| `style` | formatting (whitespace, newlines — no logic change) |
| `ci` | CI/CD config changes |
| `build` | build system or external dependency changes |

Common scopes: `cli`, `firmware-flash`, `serial-debug`, `gui`, `auth`, `ci`, `release`

```
feat(firmware-flash): add new chip manifest
fix(cli): handle missing baud rate in write command
chore: bump version to 3.1.0 [skip ci]
refactor(serial-debug): convert ws-transport dynamic import to static
ci(release): add aarch64 macOS build target
```

## Versioning

Semantic version `v<major>.<minor>.<patch>`:

| Change | Bump | Example |
|---|---|---|
| Breaking change / major rewrite | major | `v3.x.x` → `v4.0.0` |
| New feature (backward-compatible) | minor | `v3.0.x` → `v3.1.0` |
| Bug fix / hotfix | patch | `v3.0.6` → `v3.0.7` |

> v3.x: CI manages version files automatically — push tag = release.
> v2.x (master): run `node scripts/bump-version.mjs` manually before tagging.

## Release Checklist

Before pushing a v3 tag (on `refactor/v3`):

- [ ] `pnpm run lint` passes (no TypeScript/ESLint errors)
- [ ] `pnpm run build` succeeds
- [ ] `cargo test -p tyutool-core` passes
- [ ] `cargo test -p tyutool-cli` passes
- [ ] `pnpm run tauri:dev` starts correctly; core path works (select port, select firmware, flash)
- [ ] MR description or CHANGELOG lists the main changes

## Agent Behavior Guide

### Creating branches

1. Run `git status` to confirm a clean working tree
2. Choose the correct prefix based on task type (personal branch / hotfix)
3. Personal branches base off `refactor/v3`; v2 hotfixes base off `master`
4. Branch name: kebab-case English with developer prefix (`ab/`)

### Merging branches

1. Use `git rebase origin/refactor/v3` on personal branches to maintain linear history
2. Never push directly to `refactor/v3` or `master` — always use an MR
3. After a v2 hotfix MR merges into master, remind to push the patch tag

### Releasing (v3)

1. **Do not** edit version files manually — CI handles it
2. Push tag to release: `git tag v3.x.x && git push origin v3.x.x`
3. Confirm all CI jobs pass in GitHub Actions

### Releasing (v2 hotfix)

1. Run `node scripts/bump-version.mjs <version>` to update all files
2. Commit the version change, then push tag to trigger CI

### Rolling back

1. Prefer `git revert` (preserves history)
2. Never use `--force` without explicit confirmation
3. Artifact rollback: download the previous version from GitHub Releases

## Detailed Reference

Full scenario walkthroughs, conflict resolution strategies, and FAQs: [reference.md](reference.md).
