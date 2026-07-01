import { describe, expect, it } from 'vitest';
import {
  DRAFT_MARKER,
  buildDraftSection,
  insertSection,
  parseSection,
  validateCuratedSection,
} from './changelog.js';

const SKELETON = '# Changelog\n\nAll notable changes here.\n';

describe('buildDraftSection', () => {
  it('embeds the version, date, marker, and bilingual section headings', () => {
    const s = buildDraftSection('3.0.14', '2026-06-18');
    expect(s).toContain('## [3.0.14] - 2026-06-18');
    expect(s).toContain(DRAFT_MARKER);
    expect(s).toContain('### 新功能 / Features');
    expect(s).toContain('### 问题修复 / Bug Fixes');
  });
});

describe('insertSection', () => {
  it('inserts below the header when no versions exist yet', () => {
    const out = insertSection(SKELETON, '## [3.0.14] - 2026-06-18\n\nbody\n');
    expect(out).toContain('# Changelog');
    expect(out).toContain('## [3.0.14]');
    expect(out.indexOf('# Changelog')).toBeLessThan(out.indexOf('## [3.0.14]'));
  });

  it('prepends the new section above an existing one', () => {
    const existing = `${SKELETON}\n## [3.0.13] - 2026-06-01\n\nold\n`;
    const out = insertSection(existing, '## [3.0.14] - 2026-06-18\n\nnew\n');
    expect(out.indexOf('## [3.0.14]')).toBeLessThan(out.indexOf('## [3.0.13]'));
  });
});

describe('parseSection', () => {
  const cl = `${SKELETON}
## [3.0.14] - 2026-06-18

### 中文
- 修复

### English
- Fixed

## [3.0.13] - 2026-06-01

old body
`;
  it('returns the body without the heading', () => {
    const body = parseSection(cl, '3.0.14');
    expect(body).toContain('### 中文');
    expect(body).toContain('- Fixed');
    expect(body).not.toContain('## [3.0.14]');
    expect(body).not.toContain('old body');
  });
  it('returns null when the version is absent', () => {
    expect(parseSection(cl, '9.9.9')).toBeNull();
  });
});

describe('validateCuratedSection', () => {
  it('errors on null, empty, or still-marked sections; passes a real one', () => {
    expect(validateCuratedSection(null).length).toBeGreaterThan(0);
    expect(validateCuratedSection('   ').length).toBeGreaterThan(0);
    expect(validateCuratedSection(`x ${DRAFT_MARKER}`).length).toBeGreaterThan(0);
    expect(validateCuratedSection('### 中文\n- 修复')).toEqual([]);
  });
});
