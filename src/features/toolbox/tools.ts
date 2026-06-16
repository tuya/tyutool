export type ToolboxToolIcon = "batch-flash-auth";

export type ToolboxToolDef = {
  id: string;
  to: string;
  nameKey: `toolbox.${string}.name`;
  descKey: `toolbox.${string}.desc`;
  icon: ToolboxToolIcon;
};

export const TOOLBOX_TOOLS: ToolboxToolDef[] = [
  {
    id: "batch-flash-auth",
    to: "/toolbox/batch-flash-auth",
    nameKey: "toolbox.batchFlashAuth.name",
    descKey: "toolbox.batchFlashAuth.desc",
    icon: "batch-flash-auth",
  },
];
