import { describe, expect, it } from "vitest";

import { sessionUsage } from "./usage";
import type { AgentEvent, ModelUsage } from "@/types/events";

const ev = (payload: object) => ({ payload }) as AgentEvent;

function used(model: string, input: number, output: number): ModelUsage {
  return {
    model,
    inputTokens: input,
    outputTokens: output,
    cachedInputTokens: null,
    cacheWriteTokens: null,
    webSearchRequests: null,
    costUsd: null,
    contextWindow: null,
    maxOutputTokens: null,
  };
}

const turn = (perModel: ModelUsage[]) =>
  ev({ type: "turn_completed", status: "ok", usage: { perModel, rateLimit: null } });

describe("sessionUsage", () => {
  it("says nothing where the harness reported nothing", () => {
    expect(sessionUsage([])).toBeNull();
    expect(sessionUsage([ev({ type: "assistant_text", text: "hi" }), turn([])])).toBeNull();
  });

  /// `per_model` is session-cumulative, so the newest reading is the whole
  /// answer. Summing the turns would count every earlier one again.
  it("takes the newest reading whole rather than summing turns", () => {
    const usage = sessionUsage([turn([used("opus", 100, 10)]), turn([used("opus", 400, 40)])]);
    expect(usage?.totalTokens).toBe(440);
    expect(usage?.perModel).toHaveLength(1);
  });

  it("orders models by what they cost and totals them", () => {
    const usage = sessionUsage([turn([used("haiku", 10, 1), used("opus", 900, 90)])]);
    expect(usage?.perModel.map((m) => m.model)).toEqual(["opus", "haiku"]);
    expect(usage?.totalTokens).toBe(1001);
    expect(usage?.costUsd).toBeNull();
  });

  /// The live half. `usage_update` is never written to the log, so it moves the
  /// figure during a turn and is gone by the next open — which is why it is
  /// read here and why nothing rests on it being there.
  it("reads a live update the same way", () => {
    const live = ev({ type: "usage_update", perModel: [used("opus", 5, 5)], rateLimit: null });
    expect(sessionUsage([turn([used("opus", 1, 1)]), live])?.totalTokens).toBe(10);
  });

  /// A limit with no token split still draws: the plan line is the half a
  /// reader goes looking for, and Claude Code is the only harness that sends
  /// either.
  it("answers with a plan limit alone", () => {
    const limited = ev({
      type: "rate_limited",
      status: "rejected",
      resetsAt: "2026-09-23T14:00:00Z",
      limitType: "five_hour",
      overageStatus: null,
      usingOverage: true,
      overageDisabledReason: null,
    });

    const usage = sessionUsage([limited]);
    expect(usage?.perModel).toEqual([]);
    expect(usage?.limit?.usingOverage).toBe(true);
    expect(usage?.limit?.resetsAt).toBe("2026-09-23T14:00:00Z");
  });

  it("keeps the newest limit, not the first one sent", () => {
    const at = (resetsAt: string, usingOverage: boolean) =>
      ev({ type: "rate_limited", status: null, resetsAt, usingOverage });

    expect(sessionUsage([at("2026-09-23T10:00:00Z", false), at("2026-09-23T14:00:00Z", true)])?.limit)
      .toMatchObject({ resetsAt: "2026-09-23T14:00:00Z", usingOverage: true });
  });
});
