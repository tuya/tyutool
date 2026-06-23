import {
  createRouter,
  createWebHistory,
  createMemoryHistory,
  type RouteLocationNormalized,
} from "vue-router";
import { watch } from "vue";
import FirmwareFlashPage from "@/features/firmware-flash/FirmwareFlashPage.vue";
import SerialDebugPage from "@/features/serial-debug/SerialDebugPage.vue";
import BatchFlashAuthPage from "@/features/batch-flash-auth/BatchFlashAuthPage.vue";
import ToolboxPage from "@/features/toolbox/ToolboxPage.vue";
import { SettingsPage } from "@/features/settings";
import { getRuntime } from "../runtime";
import { i18n } from "../i18n";

declare module "vue-router" {
  interface RouteMeta {
    titleKey?: string;
    layout?: "fullBleed" | "default";
  }
}

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
      meta: { titleKey: "app.nav.flash", layout: "fullBleed" },
    },
    {
      path: "/serial-debug",
      name: "serial-debug",
      component: SerialDebugPage,
      meta: { titleKey: "app.nav.serialDebug", layout: "fullBleed" },
    },
    {
      path: "/toolbox",
      name: "toolbox",
      component: ToolboxPage,
      meta: { titleKey: "app.nav.toolbox", layout: "default" },
    },
    {
      path: "/toolbox/batch-flash-auth",
      name: "batch-flash-auth",
      component: BatchFlashAuthPage,
      meta: {
        titleKey: "toolbox.batchFlashAuth.name",
        layout: "fullBleed",
      },
    },
    {
      path: "/toolbox/batch-flash",
      redirect: "/toolbox/batch-flash-auth",
    },
    {
      path: "/settings",
      name: "settings",
      component: SettingsPage,
      meta: { titleKey: "app.nav.settings", layout: "default" },
    },
  ],
});

function applyTitle(route: RouteLocationNormalized) {
  const base = "tyutool";
  const key = route.meta.titleKey;
  const label = key ? (i18n.global.t(key) as string) : "";
  document.title = label ? `${label} · ${base}` : base;
}

router.afterEach(applyTitle);

// Re-resolve the title when the user switches language so the window
// title (visible in the OS taskbar / window chrome) follows the UI.
watch(
  () => i18n.global.locale.value,
  () => applyTitle(router.currentRoute.value),
);
