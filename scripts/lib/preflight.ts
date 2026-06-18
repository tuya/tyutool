export interface PreflightState {
  version: string;
  branch: string;
  expectedBranch: string;
  isClean: boolean;
  ahead: number;
  behind: number;
  tagExistsLocal: boolean;
  tagExistsRemote: boolean;
  ciStatus: string | null;
  ciConclusion: string | null;
}

export function evaluatePreEditChecks(s: PreflightState): string[] {
  const errs: string[] = [];
  if (!/^\d+\.\d+\.\d+$/.test(s.version)) {
    errs.push(`版本号格式应为 X.Y.Z，收到: ${s.version}`);
  }
  if (s.branch !== s.expectedBranch) {
    errs.push(`当前分支 ${s.branch}，应为 ${s.expectedBranch}`);
  }
  if (!s.isClean) errs.push('工作区有未提交改动，请先提交或暂存');
  if (s.behind > 0) errs.push(`落后 origin/${s.expectedBranch} ${s.behind} 个提交，请先 pull`);
  if (s.ahead > 0) errs.push(`领先 origin/${s.expectedBranch} ${s.ahead} 个未推提交，请先 push`);
  if (s.tagExistsLocal) errs.push(`本地已存在 tag v${s.version}`);
  if (s.tagExistsRemote) errs.push(`远端已存在 tag v${s.version}`);
  if (s.ciStatus !== 'completed' || s.ciConclusion !== 'success') {
    errs.push(`HEAD 的 CI 未通过 (status=${s.ciStatus ?? 'none'}, conclusion=${s.ciConclusion ?? 'none'})`);
  }
  return errs;
}
