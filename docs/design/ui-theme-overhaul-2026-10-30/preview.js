/* Standalone design proposal. Every device, log and operation is simulated. */
const state = {
  page: location.hash.slice(1) || "flash",
  locale: "zh",
  theme: "dark",
  operation: "flash",
  filter: "all",
  paused: false,
  progress: 64,
  input: "AT+GMR",
  mode: "ascii",
  crlf: true,
  settings: "appearance",
  selectedCommand: 0,
  historyQuery: "",
};
const t = (zh, en) => (state.locale === "zh" ? zh : en);
const icon = (id) => `<svg aria-hidden="true"><use href="#${id}"/></svg>`;
const escapeHtml = (text) =>
  String(text).replace(
    /[&<>"']/g,
    (c) =>
      ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[
        c
      ],
  );
const names = () => ({
  flash: t("固件工具", "Firmware tools"),
  serial: t("串口调试", "Serial debug"),
  toolbox: t("工具箱", "Toolbox"),
  settings: t("设置", "Settings"),
  batch: t("批量烧录授权", "Batch flash & auth"),
});
const heading = (eyebrow, title, description, meta = "") =>
  `<div class="page-heading"><div><div class="eyebrow">${eyebrow}</div><h1>${title}</h1><p>${description}</p></div>${meta ? `<div class="heading-meta">${meta}</div>` : ""}</div>`;
const connection =
  () => `<section class="connection" aria-label="${t("连接配置", "Connection settings")}">
  <div class="connection-state"><span class="status-dot"></span>${t("演示连接", "Demo connection")}</div>
  <div class="field-inline"><label for="port">${t("串口", "Port")}</label><select id="port"><option>COM6 · USB UART</option><option>COM8 · USB UART</option></select></div>
  <div class="field-inline"><label for="baud">${t("波特率", "Baud")}</label><select id="baud" class="baud"><option>115200</option><option>921600</option></select></div>
  <button class="plain icon-button" data-action="refresh" aria-label="${t("刷新串口", "Refresh ports")}">${icon("refresh")}</button>
  <div class="connection-spacer"></div><div class="connection-actions"><button class="plain advanced" data-action="connection-settings">${icon("sliders")}${t("连接设置", "Connection settings")}</button>
  <button data-action="disconnect" class="advanced">${t("断开连接", "Disconnect")}</button></div></section>`;
const logs = [
  ["14:32:07", "SYS", "Serial port COM6 opened at 115200 baud."],
  ["14:32:07", "OK", "Connected to Tuya T5AI."],
  ["14:32:08", "SYS", "Flash capacity: 8 MiB · Firmware: 1.24 MiB"],
  ["14:32:08", "SYS", "Erasing target sectors…"],
  ["14:32:09", "OK", "Erase completed."],
  ["14:32:09", "SYS", "Writing firmware to 0x00000000…"],
  ["14:32:10", "SYS", "Transfer active · Awaiting verification."],
];
const serialLogs = [
  ["14:32:00", "SYS", "Serial port COM6 opened at 115200 baud."],
  ["14:32:01", "RX", "[boot] TuyaOpen firmware started"],
  ["14:32:01", "RX", "[system] chip=T5AI  reset=power_on"],
  ["14:32:01", "RX", "[system] free_heap=185664  version=1.0.0"],
  ["14:32:02", "RX", "[uart] console ready · 115200 8N1"],
  ["14:32:03", "TX", "AT+GMR"],
  ["14:32:03", "RX", "TuyaOpen / T5AI / build 2026.10.30"],
  ["14:32:03", "RX", "OK"],
  ["14:32:04", "RX", "[network] interface initialized"],
  ["14:32:05", "RX", "[app] task started"],
  ["14:32:06", "RX", "[app] heartbeat=1  free_heap=183520"],
  ["14:32:07", "RX", "[app] heartbeat=2  free_heap=183520"],
];
const logRows = (rows) =>
  rows
    .map(
      ([time, level, text]) =>
        `<div class="log-row"><time>${time}</time><span class="log-level ${level.toLowerCase()}">${level}</span><code>${escapeHtml(text)}</code></div>`,
    )
    .join("");
function flashPage() {
  const operations = [
    ["flash", t("烧录", "Flash")],
    ["erase", t("擦除", "Erase")],
    ["read", t("读取", "Read")],
    ["auth", t("授权", "Authorize")],
  ];
  return (
    heading(
      "DEVICE / FIRMWARE",
      names().flash,
      t(
        "配置设备，准备固件，清晰掌握每一步。",
        "Configure the device. Prepare the image. Follow every step.",
      ),
      `<b>Tuya T5AI</b>${t("演示目标设备", "Sample target device")}`,
    ) +
    connection() +
    `<div class="split"><section class="panel"><div class="panel-head"><h2>${t("操作配置", "Operation setup")}</h2><span class="tiny mono">T5AI / 8 MiB</span></div>
    <div class="tabs" aria-label="${t("设备操作", "Device operations")}">${operations.map(([id, label]) => `<button data-operation="${id}" aria-pressed="${state.operation === id}">${label}</button>`).join("")}</div>
    <div class="panel-body"><div class="form-grid"><div class="field"><label for="chip-choice">${t("芯片型号", "Chip model")}</label><select id="chip-choice"><option>T5AI</option><option>BK7231N</option></select></div><div class="field"><label for="flash-speed">${t("烧录波特率", "Flash baud rate")}</label><select id="flash-speed"><option>921600</option><option>115200</option></select></div></div>
    <div><div class="section-label">${t("固件镜像", "Firmware image")}<span class="tiny mono muted">01</span></div><div class="file-card" style="margin-top:9px">${icon("file")}<div><strong class="mono">tuyaopen_t5ai_demo.bin</strong><small>1.24 MiB · ${t("演示固件", "Sample image")}</small></div><button data-action="choose-file">${t("更换", "Change")}</button></div></div>
    <div><div class="range"><div class="field"><label for="start-address">${t("起始地址", "Start address")}</label><input class="mono" id="start-address" value="0x00000000" /></div>${icon("arrow")}<div class="field"><label for="end-address">${t("结束地址（不含）", "End address (exclusive)")}</label><input class="mono" id="end-address" value="0x0013D000" /></div></div><div class="range-note"><span>${t("覆盖范围", "Address range")}</span><span class="mono">0x00000000 — 0x0013D000</span></div></div>
    <label class="quiet-note"><input type="checkbox" checked/>${t("写入后校验固件", "Verify firmware after writing")}</label>
    <div class="quiet-note">${icon("check")}${t("文件已就绪，地址按 4 KiB 对齐。", "Image ready. Addresses aligned to 4 KiB.")}</div></div>
    <div class="panel-actions"><span class="note">${t("演示操作，不连接实际设备", "Simulation only. No hardware access.")}</span><button class="primary ${state.operation === "erase" ? "destructive" : ""}" data-action="run-flash">${icon("play")}${t("运行演示", "Run demo")}</button></div></section>
    <section class="panel"><div class="panel-head"><h2>${t("任务进度", "Task progress")}</h2><span class="tiny">${t("演示会话", "Sample session")} <span class="mono">#001</span></span></div>
    <div class="console-top"><div><h3>${state.progress >= 100 ? t("固件写入完成", "Firmware written") : t("正在写入固件", "Writing firmware")}</h3><span class="note mono">COM6 · tuyaopen_t5ai_demo.bin</span></div><div class="progress-value">${state.progress}<small>%</small></div></div>
    <div class="console-progress"><div class="progress"><span style="width:${state.progress}%"></span></div><div class="phase-list"><span>01 ${t("连接", "Connect")}</span><span>02 ${t("擦除", "Erase")}</span><span class="active">03 ${t("写入", "Write")}</span><span>04 ${t("校验", "Verify")}</span></div></div>
    <div class="log" aria-label="${t("演示任务日志", "Sample task log")}">${logRows(logs)}${state.progress >= 100 ? logRows([["14:32:12", "OK", "Write and verification completed."]]) : ""}</div><div class="log-footer"><span>${t("任务日志", "Task log")} · ${logs.length + (state.progress >= 100 ? 1 : 0)} ${t("行", "lines")}</span><button class="plain" style="min-height:18px;font-size:12px" data-action="export">${t("导出日志", "Export log")}</button></div></section></div>`
  );
}
function serialPage() {
  const rows =
    state.filter === "all"
      ? serialLogs
      : serialLogs.filter((row) => row[2].includes(state.filter));
  return (
    heading(
      "DEVICE / SERIAL",
      names().serial,
      t(
        "让数据成为主角，连接与命令触手可及。",
        "Your data takes the stage. Connections and commands stay close.",
      ),
      `<b class="mono">115200 · 8N1</b>${t("演示会话 / UTF-8", "Sample session / UTF-8")}`,
    ) +
    connection() +
    `<section class="panel serial-console"><div class="panel-head"><div class="stream-state"><span class="status-dot"></span>${state.paused ? t("回看演示", "Demo history") : t("实时日志演示", "Live log preview")}</div><div class="console-tools"><button data-action="pause">${icon(state.paused ? "play" : "pause")}${state.paused ? t("返回实时", "Back to live") : t("暂停跟随", "Pause")}</button><button data-action="history">${icon("history")}${t("命令历史", "Commands")}</button><button data-action="export">${icon("file")}${t("导出", "Export")}</button></div></div>
    <div class="serial-filters">${[
      ["all", t("全部", "All")],
      ["[boot]", "boot"],
      ["[app]", "app"],
      ["OK", "OK"],
    ]
      .map(
        ([id, label]) =>
          `<button data-filter="${id}" aria-pressed="${state.filter === id}">${label}</button>`,
      )
      .join(
        "",
      )}<span class="connection-spacer"></span><span class="note">${t("筛选当前演示数据", "Filter sample data")}</span></div>
    <div class="log" id="serial-log" aria-label="${t("演示串口日志", "Sample serial log")}">${logRows(rows)}</div><div class="log-footer"><span>${t("本会话归档演示", "Sample session archive")} · ${rows.length} ${t("行", "lines")}</span><span>RX <span class="mono">386 B</span> <span class="status-divider">/</span> TX <span class="mono">8 B</span></span></div></section>
    <section class="sendbar" aria-label="${t("发送命令", "Send command")}"><div class="segment">${["ascii", "hex"].map((mode) => `<button data-mode="${mode}" aria-pressed="${state.mode === mode}">${mode === "ascii" ? "ASCII" : "Hex"}</button>`).join("")}</div><input id="send-input" class="mono" aria-label="${t("命令内容", "Command content")}" value="${escapeHtml(state.input)}"/><button class="icon-button plain" data-action="history" aria-label="${t("打开命令历史", "Open command history")}">${icon("history")}</button><label><input type="checkbox" id="crlf" ${state.crlf ? "checked" : ""}/>CRLF</label><button class="primary" data-action="send">${t("模拟发送", "Simulate send")}${icon("arrow")}</button></section>`
  );
}
function toolboxPage() {
  return (
    heading(
      "WORKSPACE / TOOLS",
      names().toolbox,
      t(
        "专注设备工作流，每一个工具都有清楚的位置。",
        "Focused device workflows. A clear place for every tool.",
      ),
    ) +
    `<section class="panel tool-feature"><div>${icon("box").replace("<svg ", '<svg class="tool-icon" ')}<div class="eyebrow">PRODUCTION / OPERATIONS</div><h2>${names().batch}</h2><p>${t("集中配置固件与授权数据，同时观察多个串口的任务进展。失败、隔离和重试保持清楚可见。", "Configure firmware and authorization data together. Track each port while failures, isolation and retries remain visible.")}</p><button class="primary" data-page="batch">${t("进入工作台", "Open workbench")}${icon("arrow")}</button></div>
    <div class="tool-specs"><div><span>${t("工作方式", "Workflow")}</span><strong>${t("多串口并行", "Parallel ports")}</strong></div><div><span>${t("数据输入", "Data source")}</span><strong>Excel / ${t("固件镜像", "Firmware")}</strong></div><div><span>${t("异常处理", "Recovery")}</span><strong>${t("独立状态与重试", "Per-port recovery")}</strong></div></div></section><p class="tool-footnote">${t("工具箱按当前实际功能组织；本次主题更新不增加占位工具。", "The toolbox reflects existing capabilities. No placeholder tools are added.")}</p>`
  );
}
function batchPage() {
  const metrics = [
    [t("任务进度", "Batch progress"), "24", "/ 32"],
    [t("成功", "Succeeded"), "22", ""],
    [t("需要处理", "Needs attention"), "2", ""],
    [t("正在运行", "Running"), "4", t("串口", "ports")],
  ];
  const rows = [
    ["COM6", t("写入固件", "Writing"), "64%", "64", "00:08"],
    ["COM8", t("校验固件", "Verifying"), "92%", "92", "00:11"],
    ["COM9", t("完成", "Completed"), "100%", "100", "00:12"],
    ["COM11", t("等待连接", "Waiting"), "—", "0", "—"],
    [
      "COM12",
      t("需要物理隔离", "Isolation required"),
      t("授权结果不确定", "Authorization uncertain"),
      "0",
      "00:16",
    ],
  ];
  return (
    heading(
      "TOOLBOX / PRODUCTION",
      names().batch,
      t(
        "同一套配置，每个串口都有独立且可读的状态。",
        "One configuration. An independent, readable state for every port.",
      ),
      `<b>${t("批次演示", "Sample batch")} #004</b>T5AI · USB UART`,
    ) +
    `<div class="metrics">${metrics.map(([label, value, unit], i) => `<div class="metric"><div class="metric-label">${label}</div><div class="metric-value ${i === 1 ? "green" : i === 2 ? "amber" : ""}">${value}<span>${unit}</span></div></div>`).join("")}</div>
    <section class="panel"><div class="batch-config">${icon("file")}<div><b style="font-size:12px">${t("批次配置", "Batch setup")}</b><br/><code>T5AI · tuyaopen_t5ai_demo.bin · auth-demo.xlsx</code></div><button class="plain" data-action="batch-config">${t("查看配置", "View setup")}</button><button class="primary" data-action="batch-start">${icon("play")}${t("运行批次演示", "Run batch demo")}</button></div></section>
    <section class="panel" style="flex:1"><div class="panel-head"><h2>${t("端口任务", "Port tasks")}</h2><span class="tiny">${t("固定列对齐 · 异常行展开说明", "Aligned columns · Expanded exception details")}</span></div><div class="table-scroll"><table class="batch-table"><thead><tr><th>${t("串口", "Port")}</th><th>${t("状态", "Status")}</th><th>${t("进度 / 说明", "Progress / Details")}</th><th>${t("用时", "Elapsed")}</th><th>${t("操作", "Action")}</th></tr></thead><tbody>${rows.map(([port, status, label, progress, time], i) => `<tr class="${i === 4 ? "warning-row" : ""}"><td class="mono">${port}<small>USB UART</small></td><td class="${i === 4 ? "amber" : i === 2 ? "green" : ""}">${status}</td><td><span class="mono">${label}</span>${i < 3 ? `<div class="progress"><span style="width:${progress}%"></span></div>` : i === 4 ? `<small>${t("先隔离设备，再手动核验。", "Isolate the device, then verify manually.")}</small>` : ""}</td><td class="mono muted">${time}</td><td><button data-action="${i === 4 ? "isolation" : "port-details"}">${i === 4 ? t("查看处理", "Review") : t("详情", "Details")}</button></td></tr>`).join("")}</tbody></table></div></section>`
  );
}
function themeCard(theme) {
  return `<button class="theme-card" data-theme-choice="${theme}" aria-pressed="${state.theme === theme}"><div class="theme-mini mini-${theme}"><div class="mini-rail"><i></i><i></i><i></i></div><div class="mini-main"><i></i><b></b><i></i></div></div><span class="theme-card-label">${theme === "dark" ? t("石墨 · Graphite", "Graphite") : t("纸白 · Porcelain", "Porcelain")}${state.theme === theme ? icon("check") : ""}</span></button>`;
}
function settingsPage() {
  const tabs = [
    ["appearance", t("外观与语言", "Appearance")],
    ["ports", t("串口管理", "Serial ports")],
    ["logs", t("诊断日志", "Diagnostics")],
    ["updates", t("更新", "Updates")],
    ["about", t("关于", "About")],
  ];
  let body = `<div><h2 class="settings-title">${t("主题外观", "Theme appearance")}</h2><p class="settings-desc">${t("同一套层次与操作规则，为不同光线环境设计。", "The same hierarchy and controls, designed for different lighting.")}</p><div class="theme-cards">${themeCard("light")}${themeCard("dark")}</div></div><section class="panel"><div class="setting-row"><div><b>${t("跟随系统外观", "Follow system appearance")}</b><small>${t("在正式应用中保留现有系统主题偏好。", "The existing system preference will remain available in the app.")}</small></div><button data-action="system-theme">${t("预览系统主题", "Preview system theme")}</button></div><div class="setting-row"><div><b>${t("界面语言", "Language")}</b><small>${t("中英文使用同一布局与文字层级。", "Both languages use the same layout and type hierarchy.")}</small></div><select id="locale-choice" aria-label="Language"><option value="zh" ${state.locale === "zh" ? "selected" : ""}>简体中文</option><option value="en" ${state.locale === "en" ? "selected" : ""}>English</option></select></div><div class="setting-row"><div><b>${t("减少动态效果", "Reduced motion")}</b><small>${t("样稿遵循系统偏好，不使用持续闪烁与发光。", "Respects system preferences. No continuous blinking or glow.")}</small></div><span class="note">${t("遵循系统", "System preference")}</span></div></section>`;
  if (state.settings === "ports")
    body = `<h2 class="settings-title">${t("串口管理", "Serial port management")}</h2><p class="settings-desc">${t("统一管理端口显示与活动状态。", "Manage port visibility and activity indicators together.")}</p><section class="panel">${["COM6", "COM8", "COM11"].map((port) => `<div class="setting-row"><div><b class="mono">${port}</b><small>USB UART · ${t("演示端口", "Sample port")}</small></div><label><input type="checkbox" checked/> ${t("显示", "Visible")}</label></div>`).join("")}</section>`;
  if (state.settings === "logs")
    body = `<h2 class="settings-title">${t("诊断日志", "Diagnostic logs")}</h2><p class="settings-desc">${t("诊断信息与设备会话日志保持独立。", "Diagnostics stay separate from device session logs.")}</p><section class="panel"><div class="setting-row"><div><b>${t("日志记录", "Logging")}</b><small>${t("用于定位应用故障。", "Helps diagnose application problems.")}</small></div><label><input type="checkbox" checked/>${t("启用", "Enabled")}</label></div><div class="setting-row"><div><b>${t("日志级别", "Log level")}</b></div><select aria-label="Log level"><option>Debug</option><option>Info</option></select></div><div class="setting-row"><div><b>${t("日志文件", "Log files")}</b><small>tyutool-2026-10-30.log</small></div><button data-action="export">${t("导出诊断包", "Export diagnostics")}</button></div></section>`;
  if (state.settings === "updates")
    body = `<h2 class="settings-title">${t("应用更新", "Application updates")}</h2><p class="settings-desc">${t("更新作为普通设置项呈现，避免压过日常操作。", "Updates stay part of settings, leaving everyday work in focus.")}</p><section class="panel"><div class="setting-row"><div><b>tyutool <span class="mono">3.2.10</span></b><small>${t("当前分支版本，不表示最新发布版本。", "Current branch version, not a latest-release claim.")}</small></div><button data-action="check-update">${t("模拟检查", "Simulate check")}</button></div><div class="setting-row"><div><b>${t("自动检查间隔", "Automatic check interval")}</b></div><select aria-label="Update interval"><option>${t("每天", "Daily")}</option><option>${t("每周", "Weekly")}</option></select></div></section>`;
  if (state.settings === "about")
    body = `<h2 class="settings-title">tyutool</h2><p class="settings-desc">${t("固件烧录与嵌入式设备调试工具。", "Firmware flashing and embedded device debugging.")}</p><section class="panel"><div class="setting-row"><b>${t("版本", "Version")}</b><code>3.2.10</code></div><div class="setting-row"><b>${t("主题方向", "Visual direction")}</b><span>Precision Workbench</span></div><div class="setting-row"><b>${t("设计提案", "Design proposal")}</b><code>2026-10-30 / 01</code></div></section>`;
  return (
    heading(
      "WORKSPACE / PREFERENCES",
      names().settings,
      t(
        "把常用设置组织清楚，让选择更轻松。",
        "Organized preferences. Clearer choices.",
      ),
    ) +
    `<div class="settings-layout"><nav class="settings-nav" aria-label="Settings sections">${tabs.map(([id, label]) => `<button data-settings="${id}" aria-pressed="${state.settings === id}">${label}</button>`).join("")}</nav><div class="settings-content">${body}</div></div>`
  );
}
const commands = [
  {
    name: ["查询固件版本", "Firmware version"],
    text: "AT+GMR",
    mode: "ascii",
    crlf: true,
  },
  {
    name: ["读取设备状态", "Device status"],
    text: "55 AA 00 01 00 00 00",
    mode: "hex",
    crlf: false,
  },
  {
    name: ["查看内存", "Memory status"],
    text: "free",
    mode: "ascii",
    crlf: true,
  },
];
function renderHistory(query = state.historyQuery) {
  const panel = document.querySelector("#history-panel");
  const current = commands[state.selectedCommand];
  panel.innerHTML = `<div class="inspector-head"><h2>${t("命令历史", "Command history")}</h2><button class="plain icon-button" data-action="close-history" aria-label="${t("关闭历史", "Close history")}">${icon("close")}</button></div><input type="search" id="history-search" placeholder="${t("搜索内容或名称…", "Search commands or names…")}" aria-label="${t("搜索命令", "Search commands")}" value="${escapeHtml(query)}"/><div id="command-list">${commands.map((command, index) => `${(t(...command.name) + command.text).toLowerCase().includes(query.toLowerCase()) ? `<button class="command" data-command="${index}" aria-pressed="${state.selectedCommand === index}"><strong>${t(...command.name)}</strong><small class="mono">${command.text} · ${command.mode === "hex" ? "Hex" : "ASCII / CRLF"}</small></button>` : ""}`).join("")}</div><div class="command-preview"><div class="section-label">${t("完整预览", "Full preview")}${icon("star")}</div><pre>${escapeHtml(current.text)}</pre><span class="note">${current.mode === "hex" ? "Hex" : "ASCII · CRLF"} · ${t("演示收藏命令", "Sample favorite command")}</span><button class="primary" data-action="fill-command">${t("填入发送框", "Fill send input")}${icon("arrow")}</button><p class="note">${t("填入后恢复发送参数，不自动发送。", "Restores send settings without sending.")}</p></div>`;
}
const translationPairs = {
  concept: ["视觉概念预览", "Visual concept preview"],
  sample: ["全部为演示数据", "All data is simulated"],
  workspace: ["工作空间", "WORKSPACE"],
  flash: ["固件工具", "Firmware tools"],
  serial: ["串口调试", "Serial debug"],
  toolbox: ["工具箱", "Toolbox"],
  settings: ["设置", "Settings"],
  light: ["纸白", "Porcelain"],
  dark: ["石墨", "Graphite"],
  sampleDevice: ["演示设备", "Demo device"],
  ready: ["演示工作台就绪", "Demo workbench ready"],
};
function render() {
  if (!["flash", "serial", "toolbox", "batch", "settings"].includes(state.page))
    state.page = "flash";
  document.documentElement.dataset.theme = state.theme;
  document.documentElement.lang = state.locale === "zh" ? "zh-CN" : "en";
  document.querySelectorAll("[data-i18n]").forEach((node) => {
    node.textContent = t(...translationPairs[node.dataset.i18n]);
  });
  document
    .querySelectorAll("[data-theme-choice]")
    .forEach((node) =>
      node.setAttribute(
        "aria-pressed",
        String(node.dataset.themeChoice === state.theme),
      ),
    );
  document.querySelector('[data-action="locale"]').textContent =
    state.locale === "zh" ? "EN" : "中";
  document.querySelectorAll(".rail [data-page]").forEach((node) => {
    if (node.dataset.page === (state.page === "batch" ? "toolbox" : state.page))
      node.setAttribute("aria-current", "page");
    else node.removeAttribute("aria-current");
  });
  document.querySelector("#breadcrumb").textContent =
    state.page === "batch"
      ? `${names().toolbox} / ${names().batch}`
      : `${t("工作空间", "Workspace")} / ${names()[state.page]}`;
  document.querySelector("#content").innerHTML = {
    flash: flashPage,
    serial: serialPage,
    toolbox: toolboxPage,
    batch: batchPage,
    settings: settingsPage,
  }[state.page]();
  if (!document.querySelector("#history-panel").hidden) renderHistory();
}
let toastTimer;
function toast(text) {
  const node = document.querySelector("#toast");
  node.textContent = text;
  node.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => {
    node.hidden = true;
  }, 3500);
}
let historyOpener;
function closeHistory() {
  document.querySelector("#history-panel").hidden = true;
  if (historyOpener?.isConnected) historyOpener.focus();
  else document.querySelector('.sendbar [data-action="history"]')?.focus();
}
let modalOpener;
function dialog(title, body, confirm, action) {
  const node = document.querySelector("#confirm-dialog");
  modalOpener = document.activeElement;
  node.innerHTML = `<h2 id="confirm-title">${title}</h2><p>${body}</p><div class="dialog-actions"><button autofocus data-action="cancel-dialog">${t("取消", "Cancel")}</button><button class="primary" data-action="${action}">${confirm}</button></div>`;
  node.showModal();
}
document
  .querySelector("#confirm-dialog")
  .addEventListener("close", () => modalOpener?.focus());
document.addEventListener("click", (event) => {
  const button = event.target.closest("button");
  if (!button) return;
  if (button.dataset.page) {
    state.page = button.dataset.page;
    document.querySelector("#history-panel").hidden = true;
    history.replaceState(null, "", `#${state.page}`);
    render();
    return;
  }
  if (button.dataset.themeChoice) {
    state.theme = button.dataset.themeChoice;
    render();
    return;
  }
  if (button.dataset.operation) {
    state.operation = button.dataset.operation;
    render();
    toast(
      t(
        "操作已切换；此样稿仅演示视觉层级。",
        "Operation selected. This concept illustrates visual hierarchy.",
      ),
    );
    return;
  }
  if (button.dataset.filter) {
    state.filter = button.dataset.filter;
    render();
    return;
  }
  if (button.dataset.mode) {
    state.mode = button.dataset.mode;
    render();
    return;
  }
  if (button.dataset.settings) {
    state.settings = button.dataset.settings;
    render();
    return;
  }
  if (button.dataset.command) {
    state.selectedCommand = Number(button.dataset.command);
    renderHistory();
    document
      .querySelector(`[data-command="${state.selectedCommand}"]`)
      ?.focus();
    return;
  }
  const action = button.dataset.action;
  if (action === "locale") {
    state.locale = state.locale === "zh" ? "en" : "zh";
    render();
  } else if (action === "history") {
    const panel = document.querySelector("#history-panel");
    historyOpener = button;
    panel.hidden = !panel.hidden;
    if (!panel.hidden) {
      renderHistory();
      document.querySelector("#history-search").focus();
    }
  } else if (action === "close-history") closeHistory();
  else if (action === "fill-command") {
    const current = commands[state.selectedCommand];
    state.input = current.text;
    state.mode = current.mode;
    state.crlf = current.crlf;
    document.querySelector("#history-panel").hidden = true;
    render();
    document.querySelector("#send-input")?.focus();
  } else if (action === "pause") {
    state.paused = !state.paused;
    render();
  } else if (action === "send") {
    if (!state.input.trim()) {
      toast(t("先输入演示命令。", "Enter a sample command first."));
      return;
    }
    toast(
      t(
        "已模拟发送，未访问实际串口。",
        "Send simulated. No serial port was accessed.",
      ),
    );
    serialLogs.push(["14:32:08", "TX", state.input]);
    state.input = "";
    render();
  } else if (action === "run-flash")
    dialog(
      t("运行操作演示", "Run operation demo"),
      t(
        "这里会模拟完成任务，仅用于查看进度与结果的视觉反馈，不会连接、擦除或写入真实设备。",
        "This simulates a completed task to preview progress and results. No device will be connected, erased or written.",
      ),
      t("开始演示", "Start demo"),
      "confirm-flash",
    );
  else if (action === "confirm-flash") {
    document.querySelector("#confirm-dialog").close();
    state.progress = 100;
    render();
    toast(t("演示任务完成。", "Demo task completed."));
  } else if (action === "cancel-dialog")
    document.querySelector("#confirm-dialog").close();
  else if (action === "connection-settings")
    dialog(
      t("连接设置", "Connection settings"),
      t(
        "115200 baud · 8 数据位 · 无校验 · 1 停止位。正式实现会在这里展示现有连接配置与自动释放选项。",
        "115200 baud · 8 data bits · No parity · 1 stop bit. The implementation will show existing connection settings and auto-release options here.",
      ),
      t("已了解", "Got it"),
      "cancel-dialog",
    );
  else if (action === "isolation")
    dialog(
      t("设备隔离与核验", "Device isolation and verification"),
      t(
        "COM12 的授权结果不确定。先物理隔离设备，再手动核验，避免混入已确认成功的设备。样稿仅演示警告信息的层级。",
        "COM12 has an uncertain authorization result. Physically isolate it, then verify manually before mixing it with confirmed devices. This previews warning hierarchy only.",
      ),
      t("已了解", "Got it"),
      "cancel-dialog",
    );
  else if (action === "system-theme") {
    state.theme = matchMedia("(prefers-color-scheme: dark)").matches
      ? "dark"
      : "light";
    render();
  } else if (action === "export")
    toast(
      t(
        "导出入口预览；样稿不写入文件。",
        "Export preview. This concept does not write files.",
      ),
    );
  else if (action === "choose-file")
    toast(
      t(
        "文件选择入口预览；当前使用演示固件。",
        "File chooser preview. A sample image is shown.",
      ),
    );
  else if (action === "check-update")
    toast(
      t(
        "检查入口预览；没有请求发布服务器。",
        "Check preview. No release server was contacted.",
      ),
    );
  else if (action === "batch-config")
    toast(
      t(
        "批次配置：T5AI / 演示固件 / 演示授权表。",
        "Batch setup: T5AI / Sample image / Sample authorization sheet.",
      ),
    );
  else if (action === "batch-start")
    toast(
      t(
        "批次演示已就绪，未执行真实烧录或授权。",
        "Batch demo ready. No flashing or authorization was performed.",
      ),
    );
  else if (action === "port-details")
    toast(
      t(
        "当前行展示的是该串口的演示任务状态。",
        "This row shows the sample task state for its port.",
      ),
    );
  else if (action === "refresh" || action === "disconnect")
    toast(
      t(
        "连接控件演示，未访问真实设备。",
        "Connection control preview. No hardware was accessed.",
      ),
    );
});
document.addEventListener("input", (event) => {
  if (event.target.id === "send-input") state.input = event.target.value;
  if (event.target.id === "history-search") {
    const query = event.target.value.toLowerCase();
    state.historyQuery = event.target.value;
    document.querySelectorAll("[data-command]").forEach((node) => {
      const command = commands[Number(node.dataset.command)];
      node.hidden = !(t(...command.name) + command.text)
        .toLowerCase()
        .includes(query);
    });
  }
});
document.addEventListener("change", (event) => {
  if (event.target.id === "locale-choice") {
    state.locale = event.target.value;
    render();
  }
  if (event.target.id === "crlf") state.crlf = event.target.checked;
});
document.addEventListener("keydown", (event) => {
  if (event.isComposing) return;
  const modal = document.querySelector("#confirm-dialog");
  if (modal.open && event.key === "Tab") {
    const buttons = [...modal.querySelectorAll("button")];
    const first = buttons[0];
    const last = buttons.at(-1);
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  }
  if (
    event.key === "Escape" &&
    !document.querySelector("#history-panel").hidden
  ) {
    event.preventDefault();
    closeHistory();
  }
  if (event.key === "Enter" && event.target.id === "send-input") {
    event.preventDefault();
    document.querySelector('[data-action="send"]')?.click();
  }
});
window.addEventListener("hashchange", () => {
  state.page = location.hash.slice(1) || "flash";
  document.querySelector("#history-panel").hidden = true;
  render();
});
render();
