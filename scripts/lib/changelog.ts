export const DRAFT_MARKER = '<!-- 润色后删除本行 / remove this line after editing -->';

const HEADER = '# Changelog\n\n本项目所有重要变更记录于此 / All notable changes are documented here.\n';

/**
 * Build the bilingual draft section for `version`.
 *
 * `cliffBody` is the English body git-cliff derives from conventional commits
 * (see cliff.toml). When supplied it replaces the empty English skeleton — it
 * already carries its own group headings — leaving only the Chinese half to
 * write by hand. Omitted (e.g. from `pnpm version:set`), both halves are empty.
 */
export function buildDraftSection(version: string, date: string, cliffBody?: string): string {
  const english = cliffBody?.trim()
    ? cliffBody.trim().split('\n')
    : ['### Features', '', '- ', '', '### Bug Fixes', '', '- '];

  return [
    `## [${version}] - ${date}`,
    '',
    DRAFT_MARKER,
    '',
    '### 新功能',
    '',
    '- ',
    '',
    '### 问题修复',
    '',
    '- ',
    '',
    '---',
    '',
    ...english,
    '',
  ].join('\n');
}

export function insertSection(changelog: string, section: string): string {
  const content = changelog.trim() ? changelog : HEADER;
  const lines = content.split('\n');
  let insertAt = -1;
  for (let i = 0; i < lines.length; i++) {
    if (lines[i].startsWith('## [')) {
      insertAt = i;
      break;
    }
  }
  const block = `${section.trimEnd()}\n`;
  if (insertAt === -1) {
    return `${content.replace(/\s*$/, '')}\n\n${block}`;
  }
  const before = lines.slice(0, insertAt).join('\n').replace(/\s*$/, '');
  const after = lines.slice(insertAt).join('\n');
  return `${before}\n\n${block}\n${after}`;
}

export function parseSection(changelog: string, version: string): string | null {
  const lines = changelog.split('\n');
  const start = lines.findIndex((l) => l.startsWith(`## [${version}]`));
  if (start === -1) return null;
  let end = lines.length;
  for (let i = start + 1; i < lines.length; i++) {
    if (lines[i].startsWith('## [')) {
      end = i;
      break;
    }
  }
  return lines.slice(start + 1, end).join('\n').trim();
}

export function validateCuratedSection(sectionBody: string | null): string[] {
  if (sectionBody === null) return ['CHANGELOG.md 缺少该版本小节'];
  const errs: string[] = [];
  if (sectionBody.trim() === '') errs.push('CHANGELOG.md 该版本小节为空');
  if (sectionBody.includes(DRAFT_MARKER)) {
    errs.push('CHANGELOG.md 该版本小节仍含未润色标记（请删除标记并润色后重试）');
  }
  return errs;
}
