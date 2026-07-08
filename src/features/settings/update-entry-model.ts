export interface UpdateEntryModel {
  panelTitleKey: "settings.updateCenterTitle";
  panelBodyKey: "settings.updateCenterBody";
  versionLabelKey: "settings.updateCenterVersionLabel";
  titleKey: "settings.checkUpdate";
  subtitleKey: "settings.updateEntryHint";
  metaLabelKey: "settings.updateEntryMetaLabel";
  badge: string;
}

export function buildUpdateEntryModel(version: string): UpdateEntryModel {
  return {
    panelTitleKey: "settings.updateCenterTitle",
    panelBodyKey: "settings.updateCenterBody",
    versionLabelKey: "settings.updateCenterVersionLabel",
    titleKey: "settings.checkUpdate",
    subtitleKey: "settings.updateEntryHint",
    metaLabelKey: "settings.updateEntryMetaLabel",
    badge: `v${version}`,
  };
}
