export type AppNavItemName = "flash" | "serial-debug" | "toolbox" | "settings";

export type AppNavCustomIcon = "serial-port";

export type AppNavItemDef = {
  name: AppNavItemName;
  to: string;
  faIcon?: [string, string];
  customIcon?: AppNavCustomIcon;
  labelKey: `app.nav.${string}`;
};

export const APP_NAV_ITEMS: AppNavItemDef[] = [
  {
    name: "flash",
    to: "/flash",
    labelKey: "app.nav.flash",
    faIcon: ["fas", "microchip"],
  },
  {
    name: "serial-debug",
    to: "/serial-debug",
    labelKey: "app.nav.serialDebug",
    customIcon: "serial-port",
  },
  {
    name: "toolbox",
    to: "/toolbox",
    labelKey: "app.nav.toolbox",
    faIcon: ["fas", "toolbox"],
  },
  {
    name: "settings",
    to: "/settings",
    labelKey: "app.nav.settings",
    faIcon: ["fas", "gear"],
  },
];

export function isNavItemActive(path: string, item: AppNavItemDef): boolean {
  if (item.name === "toolbox") return path.startsWith("/toolbox");
  return path === item.to;
}
