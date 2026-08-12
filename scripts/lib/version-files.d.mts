// Types for version-files.mjs — the module stays plain .mjs so bare `node` can
// load it in release.yml; this file only restores editor support for the .ts
// entry point (scripts/*.ts is not type-checked in CI).

export declare const VERSION_FILES: ReadonlyArray<{
  path: string;
  kind: 'json' | 'cargo';
}>;

export declare function applyJsonVersion(content: string, version: string): string;

export declare function applyCargoVersion(content: string, version: string): string;

export declare function readCurrentVersion(): string;

export declare function syncVersionFiles(version: string): string[];
