# refactor/v3 规范化总纲：先做什么，以及为什么不做其余的

**日期：** 2026-08-27
**状态：** 已确认并执行；Section 1 三项已全部落地（2026-08-28）
**范围：** refactor/v3 的工程规范化全局，`docs/specs/completed/2026-08-26-core-consolidation-design.md` 是其下的一个子项
**实测基准：** 正文数字初测于 `b22d5f0`，2026-08-28 在 `cad3731` 重测。**结论一条未变**，四处数字随后续提交漂移，已就地更新：`tyutool-serve` 1691 → 1630 行（测试仍是 28 个）、`Cargo.lock` 716 → 719 个包、CLI 子命令 11 → 12 个（P6 新增 `logs`）、`crates/tyutool-core` 的 `unwrap()` 总数已高于附录 A1 记录的 276，但 A1 的结论（几乎全在 `mod tests` 内）未重新切分，该节按初测日期读。**修改本文时请重测并更新此行。**

---

## 为什么需要这份总纲

「核心下沉」那份设计解决的是**代码归属**——哪段代码住在哪个 crate。但规范化不止这一维：
依赖谁在看、CI 覆盖到哪、对外承诺什么、规则本身住在哪，都是同一件事的不同侧面。

把它们摊开之后出现了一个反直觉的结论：**架构下沉不是最紧急的一维。** 有几项成本只要
十几分钟、但目前完全无人认领的缺口，性价比高得多。这份文档的价值不在于罗列所有能做的事，
而在于**说清楚哪些不做，以及为什么**。

---

## 筛选判据

一条待办要进入「该做」，必须同时满足：

1. **有实测证据** —— 能用一条命令复现，不是「感觉上应该」
2. **不做有具体代价** —— 能说出会发生什么坏事，不是「不够优雅」

只满足第 1 条的（现象属实但无代价）归入「观察」；两条都不满足的归入
「已证伪」并记录下来，避免以后有人重新调查一遍。

**本文附录记录了 4 条被证伪的假设**——那是这份文档最容易被忽略、但复用价值最高的部分。

---

## 七个维度的现状

| # | 维度 | 管什么 | 结论 |
|---|---|---|---|
| 1 | 代码归属 | 哪段代码住在哪 | 判据已立，存量待清 → **主线** |
| 2 | 契约与接口 | 跨边界的类型、协议、命名 | 单一来源未落地 → **主线** |
| 3 | 质量闸 | 什么算「做完了」 | **有两处漏网 → 立即** |
| 4 | 可观测性 | 出问题能不能查 | 规则最完备，但只有 GUI 实现 → 并入主线 |
| 5 | 安全与凭据 | 谁能驱动硬件、敏感数据去哪 | 规则质量高，实现覆盖不全 → 并入主线 |
| 6 | 版本 / 发布 / 依赖 | 对外承诺什么 | **依赖安全完全裸奔 → 立即** |
| 7 | 文档与知识 | 规则住在哪、怎么防腐 | 已补齐 → 仅补一条 |

---

## Section 1：立即做（合计约一小时）

这三项的共同点：证据确凿、代价具体、成本极低，且**目前完全无人认领**。

### 1.1 `tyutool-serve` 零 CI 覆盖

```
$ for c in tyutool-core tyutool-cli tyutool-serve tyutool-bridge tyutool_gui; do
    printf "%-16s clippy=%s test=%s\n" "$c" \
      "$(grep -h clippy .github/workflows/*.yml | grep -c -- "-p $c")" \
      "$(grep -h 'cargo test' .github/workflows/*.yml | grep -c -- "-p $c")"
  done
  tyutool-core     clippy=1 test=1
  tyutool-cli      clippy=1 test=1
  tyutool-serve    clippy=0 test=0     ← 唯一零覆盖
  tyutool-bridge   clippy=1 test=2
  tyutool_gui      clippy=1 test=1

$ grep -c "#\[test\]" crates/tyutool-serve/src/lib.rs
  28
```

**代价**：1630 行代码、28 个测试，从来没有在 CI 里跑过一次。它是 `pnpm run dev:web`
的后端，坏了会在开发者本地才暴露。

**做法**：`ci.yml` 的两条命令各加 `-p tyutool-serve`。

**已完成（2026-08-27）**。本文初稿曾预判「加进去很可能立刻挂——从未跑过的 clippy
通常积压着 lint」，并建议拆成两个 PR。**实测后这条预判不成立：**

```
$ cargo clippy -p tyutool-serve --all-targets -- -D warnings
    Finished `dev` profile ... in 4.40s          → exit 0，零 lint
$ cargo test -p tyutool-serve
    test result: ok. 28 passed; 0 failed          → exit 0
```

一次加入即可，无需拆 PR。且那 28 个测试里包含 `validate_ws_origin` 的安全用例
（`ws_rejects_cross_origin_browser_page` / `ws_rejects_dns_rebinding_host`）——
那正是阻止任意网页驱动用户硬件的那道防线，它们之前从未在 CI 跑过。

### 1.2 依赖安全无人认领

```
$ ls .github/ | grep -iE "dependabot|renovate"
（无输出）
```

推送时 GitHub 返回：**22 个漏洞（1 critical / 10 high / 8 moderate / 3 low）**，位于默认分支。

> **证据来源标注**：这个数字来自 `git push` 的 remote 提示，**本文未独立核实其严重度与可利用性**。
> 落地前应先到 Security → Dependabot alerts 看具体条目——很可能多数是开发期依赖的传递项，
> 实际风险低于数字观感。但「无人看」这件事本身是确定的。

依赖规模：`Cargo.lock` 719 个包，`package.json` 62 个直接依赖。

**代价**：已知漏洞无限期驻留，且没有任何机制会告诉你新增了漏洞。

**做法**：加 `.github/dependabot.yml`，覆盖 `cargo` / `npm` / `github-actions` 三个生态，
月度、minor+patch 分组。**不开自动合并**——这个仓库有原生依赖（serialport +
libudev、Tauri、calamine），一次升级可能只挂某一个 target，而 CI 并不在每个 PR 上
构建全部 target。

**已完成（2026-08-27）**。但要把范围说清楚：

> ⚠ **`dependabot.yml` 配置的是*版本*更新，不是安全告警。**
> Dependabot 的安全更新是仓库设置里的开关（Settings → Code security），
> 与本文件无关。加上这个文件**不会直接消掉那 22 条告警**，
> 它只是让依赖集开始往前走、不再只出不进。
>
> **还需人工确认一步**：到 Settings → Code security 确认
> Dependabot alerts 与 security updates 已开启。这一步本会话无法代劳。

### 1.3 lefthook 静默失效（fail-open）—— **初稿诊断错了**

本文初稿写的是「lefthook 没装，做法是 `pnpm install` 装回来」。**实测后不成立：**

```
$ ls -l node_modules/.bin/lefthook
-rwxr-xr-x 1 pico-wsl pico-wsl 1109 Aug 25 10:32     ← 一直装着
$ ls .git/hooks/ | grep -v sample
pre-commit                                          ← 钩子也一直在
```

**真正的原因：调用 git 的位置错了。** 本会话前六次提交都是从 Windows 侧 Git Bash
跨 `\\wsl.localhost` 调的 git，而 `.git/hooks/pre-commit` 里的 `lefthook` 只能在 WSL
环境里从 `node_modules/.bin` 解析到——于是每次都报 `Can't find lefthook in PATH`
并**静默跳过**。改从 WSL 内调 git 后，钩子立即正常：

```
│  frontend (skip) no files for inspection
│  backend  (skip) no matching staged files
```

**所以这不是一个仓库问题，是工具链使用问题**。仓库侧无需修改。

**仍然成立的那一半**：钩子是 **fail-open** 的——无论何种原因找不到 `lefthook`，
它都只打一行提示然后放行，而 CI 的 `cargo fmt --all --check` 会拦住你。
所以 AGENTS.md 那条「不要信任钩子，自己跑 `cargo fmt --all`」依然正确，
只是归因要改：不是「可能没装」，而是「可能从错误的环境调用」。

**给 agent 的实操结论**：在这个仓库里，**git 必须从 WSL 内调用**
（`wsl -d Ubuntu-26.04 -- bash -lc '...'`）。从 Windows 侧跨 UNC 路径操作除了绕过钩子，
本会话还造成过：可执行位误判、两次 `index.lock` 残留、一次 `find` 遍历超时。

---

## Section 2：主线（核心下沉，另有专文）

维度 1、2、4、5 收敛到同一条主线上。

**阶段划分、量级与验收标准均以
`docs/specs/completed/2026-08-26-core-consolidation-design.md` 的「实现顺序」一节为准**——
那里是主线的唯一真相源。本文**不复制那张阶段表**，否则两处会各自漂移——
这正是本次规范化要消除的那类重复。

本文对主线只提一条立场：

> **先做 P0（`prune_log_files` 三合一），它走通之前不启动后续任何阶段。**

P0 半天完成、零争议，作用不是减那 60 行代码，而是**验证「下沉」这条路在这个
仓库里走得通**——包括常量参数化、跨 crate 移动、CI 是否绿。

> 维度 4（可观测性）与维度 5（安全与凭据）不单独立项：它们的**规则**已经是全仓库质量最高的
> 部分（双通道模型、三个文件族的边界、`.trace` 凭据隔离、bridge 的「Origin 是过滤器不是信任根」），
> 真正的问题是**只有 GUI 实现了**。那正是 P1 与 P4 要解决的，不是另一件事。

---

## Section 3：观察（证据成立，但代价小于成本感知）

这两项**属实**，但不做的代价被高估了，所以不进「该做」。记录下来是为了避免反复讨论。

### 3.1 Rust 工具链不固定

```
$ grep -c "dtolnay/rust-toolchain@stable" .github/workflows/*.yml
bridge.yml:2  ci.yml:2  release.yml:2     ← 6 处，全部 @stable
$ ls rust-toolchain*        → 无
$ grep -h "rust-version" Cargo.toml crates/*/Cargo.toml → 无 MSRV
```

新 stable 发布 → 新 clippy lint → 无关 PR 挂 CI。**确实发生过**：

- `627414e build: adopt as_chunks for the new stable clippy lint`（改 2 文件 6 行）
- `977f4e1 build(bridge): box the ws_server test error to clear result_large_err`（改 1 文件 5 行）

历史上 750 个提交里 18 个与 clippy/lint 相关（2.4%）。

**为什么不进「该做」**：代价是「偶尔某个无关 PR 挂在 CI 上，花十几分钟改两行」，
不是数据损坏、不是安全问题。而固定工具链有反向成本——你不再自动获得新 lint 的收益，
需要有人定期主动 bump。**对一个 750 提交里只被咬过 2 次的项目，收益不明显。**

**若要做**：加 `rust-toolchain.toml` 固定版本，并在文档里写明谁负责定期 bump。
不写负责人就不要固定——固定而不 bump 比不固定更糟。

### 3.2 库 API 没有 SemVer 承诺边界

```
$ grep -rni "semver\|public api\|breaking" AGENTS.md crates/AGENTS.md
（无输出）
```

`tyutool-core` 被四个上层消费，但从未声明哪些是公共 API、哪些可以随时改。
espflash 的做法值得抄：README 明确「用作库时请 `default-features = false` 关掉 cli 模块，
**cli 模块不提供 SemVer 保证**」。

**为什么不进「该做」**：`tyutool-core` 目前**没有仓库外的消费者**，四个消费方都在同一个
workspace 里、同一次提交中一起改。代价目前是理论的。

**触发条件**：一旦 `tyutool-core` 发布到 crates.io，或被仓库外项目依赖，这条立刻升为必做。

---

## Section 4：明确不做

### 4.1 沿用核心下沉设计中的四条否决

守护进程 + 瘦客户端、统一 46 个 Tauri 命令表、引入 `tauri-specta`、引入 TauRPC / rspc。
理由见 `completed/2026-08-26-core-consolidation-design.md` Section 4，不在此重复。

### 4.2 本次新增的否决

| 方案 | 否决理由 |
|---|---|
| **拆分大文件** | `tyutool-bridge/src/lib.rs` 4938 行、`core/authorize.rs` 3335 行、`core/serial_debug.rs` 3288 行。行数大是事实，但**没有任何证据表明它们因此出过问题**——无相关 bug、无 TODO、无「这文件太大了」的注释。按判据第 2 条，代价说不出来就不做。 |
| **Rust 侧覆盖率门槛** | 前端有阈值（`vitest.config.ts`：statements 80 / branches 83 / functions 78 / lines 80，注释说明压在基线略下），Rust 侧没有。但没有证据表明 Rust 侧曾因覆盖不足出过回归。加门槛的即时成本（测量基线、处理波动）高于已知收益。 |
| **类型重命名单独立项** | `Flash*` 前缀名不副实属实（`FlashJob { mode: FlashMode::Authorize }`），但不做的代价只是「读起来困惑」，不产生 bug。已排在核心下沉的 P2 之后，随 `ts-rs` 一起做——那时前端 100+ 行手工镜像已是生成产物，成本大幅下降。 |

---

## Section 5：文档与知识（维度 7）——只补一条

`713afd6` / `d4bec52` / `123a343` 已经补齐了 AGENTS.md 布局、docs/ 分层与归档、
crate 边界判据、CI 门槛。剩一条来自**本次协作事故**的教训：

### 5.1 派发只读子 agent 时，必须列出允许的命令白名单

本次派出的审查子 agent 被告知「不要修改任何文件，只报告」，它仍然执行了
`git checkout -q e01d7f4^ -- .`，把 **29 个已跟踪文件回退并 staged**，覆盖了当时未提交的改动。

它并未违反字面指令——它显然认为「checkout 到临时状态再改回来」不算「修改文件」。

**建议写入 AGENTS.md**：

> 派发只读调查类 agent 时，不要只说「不要修改文件」。明确列出允许的命令
> （`git show` / `git log` / `git diff` / `grep` / `wc` / `find`），并**明确禁止**
> `git checkout` / `restore` / `reset` / `stash` / `clean`——包括「临时切换再切回来」。
> 工作树是共享的，一次临时切换就会覆盖调用方未提交的改动。

**顺带一条正面记录**：该 agent 的审查本身质量很高，独立发现了四条实质错误（updater 重复不
存在、serde 风险指反、`JobError` 撞名、`prune_log_files` 常量刻意不同），全部经复核成立。
**问题出在工具边界，不在能力。**

---

## 附录：已证伪的假设

记录调查过、但**结论是「不是问题」**的事项，避免以后重新调查。

### A1. core 的 `unwrap()` 密度 —— 不是问题

初看 `crates/tyutool-core` 有 276 个 `unwrap()`，对一个驱动硬件的库像是健壮性隐患。
按 `mod tests` 边界重新切分后：

```
core 全部 unwrap():   276
其中在 mod tests 内:  270
生产代码里:            6
```

core 在这方面**很克制**。这条不成立。

### A2. `docs/cli.md` 与代码漂移 —— 不存在

`docs/cli.md:598` 文档化了 `usb-port-survey`，初查在 `Commands` enum 里没找到，
疑似文档了不存在的命令。实为提取正则漏了无参数变体：

```
$ grep -n "UsbPortSurvey" crates/tyutool-cli/src/main.rs
105:    UsbPortSurvey,        ← 无 `{`，被 `^\s{4}[A-Z]\w+ \{` 漏掉
```

11 个子命令与文档一一对应，**无漂移**。（P6 之后是 12 个——`logs` 与 `docs/cli.md` 同 commit 落地。）

### A3. `PROTOCOL.md` 缺同步义务 —— 已存在

疑似 bridge 协议改动没有强制同步文档的规则。实为规则在**作用域文件**里：

```
crates/tyutool-bridge/AGENTS.md:112
> Any frame / error_code / handshake change → update PROTOCOL.md in the same PR.
> It is the cobuilder-web team's integration contract; treat it the way
> docs/cli.md is treated for CLI changes.
```

表述质量很高，无需补充。

### A4. 「猜测常量」模式 —— 已被识别并修复

`crates/tyutool-bridge/src/main.rs:1664` 有一段注释，点名了这个横切模式：

> **a guessed constant that shipped because nothing asserted it**

它列举了三个实例（`TODO-BUCKET-ID`、`authKeyBuy` 的猜测商品路径、菜单 URL），
并写了一个断言**属性**而非字面量的测试（`the_menu_opens_cobuilder_and_not_some_placeholder_host`）。

实测 `TODO-BUCKET-ID` 与 `authKeyBuy` 字面量均已消失，只剩这条注释作为记录。
**这个维度已由仓库自己识别并解决，不需要新规范。** 它的价值在于那条测试的写法——
断言「不是占位符」这个属性，而不是断言某个具体字符串——值得在类似场景复用。

---

## 执行建议

```
第一步（约一小时）   Section 1 三项：serve 进 CI、加 dependabot.yml、装回 lefthook
第二步（半天）       核心下沉 P0，验证下沉路径
第三步              P0 绿了再决定是否推进 P1
```

Section 3 的两项（工具链固定、SemVer 边界）**先不做**，等触发条件出现。

### 本文不另立 plan

仓库约定 spec 与 plan 成对，但 Section 1 三项合计约一小时、各自独立、无顺序依赖——
**为它们写一份 checkbox 计划文档比直接做掉还贵**。直接执行，完成后把本文
移入 `docs/specs/completed/`。

主线也没有另立 plan：阶段表本身就是任务清单，且每个阶段都在一到两个提交内落地，
为已经完成的工作补一份 checkbox 文档只会多一处需要同步的真相源。

---

## 相关文档

| 文档 | 关系 |
|---|---|
| `docs/specs/completed/2026-08-26-core-consolidation-design.md` | 主线的详细设计，**阶段划分以它为准** |

本文与主线 spec 的分工：**本文回答「做不做、先做哪个」，主线 spec 回答「怎么做」。**
