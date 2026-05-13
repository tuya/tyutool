# Serial Debug Phase 2 Design: Filter UX Redesign + ANSI Parsing

## Context

Phase 1 MVP 已交付。本 spec 覆盖 Phase 2 的两项核心改进：

1. **过滤体验重设计** — 现有的常驻 FilterBar 存在三个痛点：始终占位、三态模式理解成本高、主视图与过滤视图切换繁琐。重设计为搜索浮层模式（类似浏览器 Ctrl+F），零打扰，交互直觉。
2. **ANSI 转义序列解析** — 支持颜色（3/4-bit、256 色、RGB 真彩色）和文字样式（bold、italic、underline），其他控制序列剥除后显示纯文本。

---

## Goals

- 过滤默认不占 UI 空间；需要时一键呼出，Esc/X 关闭即清除。
- 简化过滤模式：关闭搜索条 = 关闭过滤，打开时只有 Include/Exclude 一个切换。
- 串口输出中的 ANSI 颜色和样式直接在日志里正确渲染。
- 提供关闭 ANSI 解析的开关（默认开启），关闭后显示剥除后的纯文本。

## Non-Goals

- 自定义关键词高亮规则（Phase 2 后续）。
- 正则过滤（Phase 2 后续）。
- 光标移动、清屏等终端控制序列（只剥除，不模拟终端行为）。

---

## Design

### 一、过滤体验重设计

#### 交互流程

| 状态 | UI 表现 |
|------|---------|
| 默认 | LogPane 工具栏右侧只有一个搜索图标按钮 🔍 |
| 点击 🔍 | 日志内容区顶部滑出搜索条（sticky，不随内容滚动） |
| 搜索条内容 | 输入框 + Include/Exclude 切换按钮 + 命中计数 + X 关闭 |
| 按 Esc 或 X | 清空 filterText、设 filterMode='off'、关闭搜索条 |
| Tauri 模式 | 工具栏右侧额外有"新窗口"图标按钮（web 下隐藏） |

#### 模式简化

原三态（Off / Include / Exclude）→ 两态：

- **搜索条关闭** = `filterMode = 'off'`（不过滤，全量显示）
- **搜索条开启** = `filterMode = 'include'` 或 `'exclude'`，由一个切换按钮控制，默认 include

搜索条打开时若 filterText 为空，不做过滤，但保留模式选择（用户输入时即时生效）。

#### 文件变动

| 文件 | 操作 |
|------|------|
| `src/features/serial-debug/components/SerialDebugFilterBar.vue` | **删除** |
| `src/features/serial-debug/SerialDebugPage.vue` | 移除 FilterBar import 与挂载 |
| `src/features/serial-debug/components/SerialDebugLogPane.vue` | 搜索条 + 新窗口按钮内联到工具栏 |
| `src/stores/serial-debug.ts` | 无逻辑改动，filterText/filterMode 语义不变 |

#### LogPane 工具栏布局（修改后）

```
[Log 标题] [🔍 搜索] ... [右侧: 🗗 新窗口 (Tauri only)] [⬇ 保存] [🗑 清空]
               ↓ 展开后在日志内容区顶部
[  输入框...  ] [Include | Exclude] [3 of 18] [×]
```

---

### 二、ANSI 转义序列解析

#### 支持的序列

| 类型 | 参数 |
|------|------|
| 前景色 3/4-bit | 30-37、90-97 |
| 背景色 3/4-bit | 40-47、100-107 |
| 256 色前景 | `38;5;n` |
| 256 色背景 | `48;5;n` |
| RGB 前景 | `38;2;r;g;b` |
| RGB 背景 | `48;2;r;g;b` |
| 粗体 | 1（reset: 22） |
| 斜体 | 3（reset: 23） |
| 下划线 | 4（reset: 24） |
| 全部重置 | 0 |
| 其他序列 | 剥除，不显示 escape 字符 |

#### 新文件：`src/features/serial-debug/ansi-parse.ts`

```ts
interface AnsiStyle {
  fg?: string;   // CSS 颜色值，如 '#ff0000' 或 'rgb(255,0,0)'
  bg?: string;
  bold?: boolean;
  italic?: boolean;
  underline?: boolean;
}

interface AnsiSpan {
  text: string;
  style: AnsiStyle;
}

// 纯函数，无副作用
export function parseAnsi(text: string): AnsiSpan[];

// 剥除所有 ANSI 序列，返回纯文本
export function stripAnsi(text: string): string;
```

`parseAnsi` 从左到右扫描，遇到 `\x1b[` 开始的 SGR 序列更新当前样式状态，遇到其他转义序列（非 `m` 结尾）直接跳过字符。输出连续文本片段按当前样式合并。

#### LogPane 渲染变化

- `ansiEnabled` store 状态控制是否调用 `parseAnsi`（否则调用 `stripAnsi`）。
- 每行 `.text` 从纯文本节点 → `<span v-for="span in parsedSpans(line.text)" :style="spanStyle(span)">{{ span.text }}</span>`。
- TX/RX 行背景色和方向 badge 颜色不变，ANSI 样式仅影响行内文字。
- SYS 行不做 ANSI 解析（系统消息是前端拼接的纯文本）。

#### 新增 store 状态

```ts
const ansiEnabled = ref(true);  // 持久化到 serial-debug-workspace
```

#### Settings Modal 新增控件

在 `SerialDebugSettingsModal.vue` 的数据位/校验/停止位区域下方，增加：

```
[✓] 解析 ANSI 颜色与样式
    关闭后剥除转义码，显示纯文本
```

#### i18n 新增 key

```
serialDebug.conn.ansiParse        解析 ANSI 颜色与样式 / Parse ANSI colors & styles
serialDebug.conn.ansiParseTip     关闭后剥除转义码，显示纯文本 / Disable to strip escape codes and show plain text
```

---

## File Structure

### 新增

```
src/features/serial-debug/
  ansi-parse.ts          # parseAnsi / stripAnsi 纯函数
  ansi-parse.test.ts     # 单元测试
```

### 修改

```
src/features/serial-debug/
  SerialDebugPage.vue                        # 移除 FilterBar
  components/SerialDebugLogPane.vue          # 内联搜索条 + 新窗口按钮 + ANSI 渲染
  components/SerialDebugFilterBar.vue        # 删除
  components/SerialDebugSettingsModal.vue    # 新增 ANSI 开关
src/stores/serial-debug.ts                   # 新增 ansiEnabled
src/stores/serial-debug-workspace.ts         # 持久化 ansiEnabled
src/locales/zh-CN.json                       # ansiParse/ansiParseTip
src/locales/en.json                          # 同上
```

---

## Testing

### ansi-parse.test.ts

- 纯文本（无 ANSI）→ 单一 span，无样式
- 单色序列 `\x1b[31m` → fg red，reset `\x1b[0m` 后样式清空
- 256 色 `\x1b[38;5;196m`
- RGB 真彩色 `\x1b[38;2;255;0;0m`
- 粗体+斜体+下划线组合，各自 reset 后状态正确
- 非 SGR 序列（如 `\x1b[2J`）被剥除，不出现在文本中
- `stripAnsi` 完全剥除所有转义序列

### 过滤交互（手动烟雾测试）

- 打开搜索条 → include 模式 → 输入关键词 → 日志实时过滤
- 切换 Exclude → 反向过滤
- Esc / X → 过滤清除，全部行重新显示
- Tauri 模式：工具栏新窗口按钮正常打开子窗口，过滤条件正确传递

---

## Rollout Order

1. `ansi-parse.ts` + 单元测试（TDD）
2. LogPane 过滤 UI 重构（删 FilterBar，内联搜索条）
3. LogPane ANSI 渲染
4. Settings Modal 新增 ansiEnabled 开关
5. Store + Workspace 持久化 ansiEnabled
6. i18n、手动烟雾测试
