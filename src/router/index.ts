import {
  createRouter,
  createWebHistory,
  createMemoryHistory,
} from "vue-router";
import FirmwareFlashPage from "@/features/firmware-flash/FirmwareFlashPage.vue";
import SerialDebugPage from "@/features/serial-debug/SerialDebugPage.vue";
import BatchFlashAuthPage from "@/features/batch-flash-auth/BatchFlashAuthPage.vue";
import ToolboxPage from "@/features/toolbox/ToolboxPage.vue";
import { SettingsPage } from "@/features/settings";
import { getRuntime } from "../runtime";

export const router = createRouter({
  // createMemoryHistory for VS Code webview: the vscode-webview:// URL scheme
  // confuses createWebHistory base resolution, preventing the / → /flash redirect.
  // Memory history keeps all navigation in-process and works in any URL scheme.
  history:
    getRuntime() === "vscode"
      ? createMemoryHistory()
      : createWebHistory(import.meta.env.BASE_URL),
  routes: [
    { path: "/", redirect: "/flash" },
    {
      path: "/flash",
      name: "flash",
      component: FirmwareFlashPage,
      meta: { title: "固件烧录", layout: "fullBleed" },
    },
    {
      path: "/serial-debug",
      name: "serial-debug",
      component: SerialDebugPage,
      meta: { title: "串口调试", layout: "fullBleed" },
    },
    {
      path: "/toolbox",
      name: "toolbox",
      component: ToolboxPage,
      meta: { title: "工具箱", layout: "default" },
    },
    {
      path: "/toolbox/batch-flash-auth",
      name: "batch-flash-auth",
      component: BatchFlashAuthPage,
      meta: { title: "批量烧录授权", layout: "fullBleed" },
    },
    {
      path: "/toolbox/batch-flash",
      redirect: "/toolbox/batch-flash-auth",
    },
    {
      path: "/settings",
      name: "settings",
      component: SettingsPage,
      meta: { title: "设置", layout: "default" },
    },
  ],
});

router.afterEach((to) => {
  const base = "tyutool";
  const t = to.meta.title;
  document.title = t ? `${t} · ${base}` : base;
});
