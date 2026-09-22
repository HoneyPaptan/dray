import type { AgentEvent, ModelUsage } from "@/types/events";

/// What the plan window has left, as the harness last reported it.
///
/// Only Claude Code says anything at all, and only when the news is bad — the
/// mapper drops every healthy `rate_limit_event`, so an absent reading means
/// "nothing has gone wrong", never "nothing is known about the plan". That is
/// why the picker draws this line only where one has arrived.
export type PlanLimit = {
  /// The harness's own word. `allowed` never reaches here.
  status: string | null;
  /// RFC3339, ready for [`resetTime`](./format).
  resetsAt: string | null;
  /// Requests are already being billed as usage rather than covered.
  usingOverage: boolean;
  /// Fraction of the window spent, `0.93` at 93%. Nothing fills this today —
  /// it rides `Usage.rate_limit`, which no harness maps yet — so it is read
  /// where present and drawn where read.
  usedPercent: number | null;
};

/// What a session has spent, split the way the harness splits it.
export type SessionUsage = {
  /// Session-cumulative, one entry per model the session has actually used.
  /// Newest reading wins; every entry is a running total, not a delta.
  perModel: ModelUsage[];
  /// Every token the session has been charged for, cache included.
  totalTokens: number;
  /// Summed where the harness prices its own turns, `null` where none does.
  costUsd: number | null;
  limit: PlanLimit | null;
};

function tokensOf(m: ModelUsage): number {
  return (
    (m.inputTokens ?? 0) + (m.outputTokens ?? 0) + (m.cachedInputTokens ?? 0) + (m.cacheWriteTokens ?? 0)
  );
}

/// Read what this session has spent back out of its own log.
///
/// Derived rather than tracked, the bargain the context ring already makes: the
/// numbers are persisted on `turn_completed`, so a session reopened tomorrow
/// reads exactly what it read live. `usage_update` is read too and is the live
/// half — it is never written to the log, so it moves the figure during a turn
/// and is gone by the next open.
///
/// One backward pass, and each half settles on the first event that carries it:
/// `per_model` is cumulative, so the newest reading is the whole answer, and
/// the plan limit is news that stands until it is repeated.
export function sessionUsage(events: AgentEvent[]): SessionUsage | null {
  let perModel: ModelUsage[] | null = null;
  let limit: PlanLimit | null = null;

  for (let i = events.length - 1; i >= 0 && (perModel === null || limit === null); i--) {
    const p = events[i].payload;

    if (p.type === "rate_limited") {
      limit ??= {
        status: p.status,
        resetsAt: p.resetsAt,
        usingOverage: p.usingOverage,
        usedPercent: null,
      };
      continue;
    }

    const usage = p.type === "turn_completed" ? p.usage : p.type === "usage_update" ? p : null;
    if (!usage) continue;

    if (perModel === null && usage.perModel.length > 0) perModel = usage.perModel;
    if (limit === null && usage.rateLimit?.usedPercent != null) {
      limit = {
        status: null,
        resetsAt: usage.rateLimit.resetsAt,
        usingOverage: false,
        usedPercent: usage.rateLimit.usedPercent,
      };
    }
  }

  if (perModel === null && limit === null) return null;

  const models = perModel ?? [];
  const priced = models.filter((m) => m.costUsd != null);

  return {
    perModel: [...models].sort((a, b) => tokensOf(b) - tokensOf(a)),
    totalTokens: models.reduce((sum, m) => sum + tokensOf(m), 0),
    costUsd: priced.length > 0 ? priced.reduce((sum, m) => sum + (m.costUsd ?? 0), 0) : null,
    limit,
  };
}

/// Tokens on one model, for the row that draws it. Exported so the picker and
/// the total cannot count a model two different ways.
export const modelTokens = tokensOf;
