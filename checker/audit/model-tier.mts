import { env } from 'node:process';
import type { ReasoningEffort } from '../fx/index.mts';

const DEFAULT_STANDARD_MODEL = 'openai/gpt-6-luna';
const DEFAULT_DEEP_MODEL = 'openai/gpt-6.1-terra';

type AuditTier = Readonly<{
  effort: ReasoningEffort;
  model: string;
}>;

type SelectAuditTierInput = Readonly<{
  agentModel?: string;
  auditModel?: string;
  deep: boolean;
}>;

function readModel({ agentModel, auditModel, deep }: SelectAuditTierInput): string {
  if (auditModel !== undefined && auditModel.length > 0) {
    return auditModel;
  }

  if (agentModel !== undefined && agentModel.length > 0) {
    return agentModel;
  }

  if (deep) {
    return DEFAULT_DEEP_MODEL;
  }

  return DEFAULT_STANDARD_MODEL;
}

function readEffort(deep: boolean): ReasoningEffort {
  if (deep) {
    return 'medium';
  }

  return 'low';
}

/** Choose the model and reasoning effort for audit contract synthesis and witness review. */
export function selectAuditTier({
  agentModel = env.TENET_AGENT_MODEL,
  auditModel = env.TENET_AUDIT_MODEL,
  deep,
}: SelectAuditTierInput): AuditTier {
  const model = readModel({ agentModel, auditModel, deep });
  const effort = readEffort(deep);

  return { effort, model };
}
