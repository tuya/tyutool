import { describe, expect, it } from 'vitest';
import { evaluatePreEditChecks, type PreflightState } from './preflight.js';

const OK: PreflightState = {
  version: '3.0.14',
  branch: 'refactor/v3',
  expectedBranch: 'refactor/v3',
  isClean: true,
  ahead: 0,
  behind: 0,
  tagExistsLocal: false,
  tagExistsRemote: false,
  ciStatus: 'completed',
  ciConclusion: 'success',
};

describe('evaluatePreEditChecks', () => {
  it('passes a clean, green, in-sync state', () => {
    expect(evaluatePreEditChecks(OK)).toEqual([]);
  });
  it('rejects a bad version format', () => {
    expect(evaluatePreEditChecks({ ...OK, version: 'v3.0' }).length).toBeGreaterThan(0);
  });
  it('rejects the wrong branch', () => {
    expect(evaluatePreEditChecks({ ...OK, branch: 'main' }).length).toBeGreaterThan(0);
  });
  it('rejects a dirty tree', () => {
    expect(evaluatePreEditChecks({ ...OK, isClean: false }).length).toBeGreaterThan(0);
  });
  it('rejects being behind or ahead of origin', () => {
    expect(evaluatePreEditChecks({ ...OK, behind: 2 }).length).toBeGreaterThan(0);
    expect(evaluatePreEditChecks({ ...OK, ahead: 1 }).length).toBeGreaterThan(0);
  });
  it('rejects an existing tag (local or remote)', () => {
    expect(evaluatePreEditChecks({ ...OK, tagExistsLocal: true }).length).toBeGreaterThan(0);
    expect(evaluatePreEditChecks({ ...OK, tagExistsRemote: true }).length).toBeGreaterThan(0);
  });
  it('rejects CI that is not completed+success', () => {
    expect(evaluatePreEditChecks({ ...OK, ciStatus: 'in_progress', ciConclusion: null }).length).toBeGreaterThan(0);
    expect(evaluatePreEditChecks({ ...OK, ciConclusion: 'failure' }).length).toBeGreaterThan(0);
  });
});
