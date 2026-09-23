import type { Effort, Harness, Model, ModelId } from "@/types/events";

/// The one spelling of "nothing here can name the model".
///
/// Written by an index entry made before the model was known, and by an older
/// build that read an id it did not recognise. `models.rs` holds the same
/// constant and normalises the older `"unknown"` onto it on the way in, so this
/// is the only spelling that reaches the frontend.
///
/// **Never drawn.** It is not a model name and there is no name to draw — a
/// surface holding one shows the picker's own placeholder instead.
export const UNSET_MODEL = "" as ModelId;

export function isUnsetModel(id: ModelId): boolean {
  return id === UNSET_MODEL;
}

/// What each harness opens on before its reader has picked anything, and what a
/// session indexed without a model reads back as.
///
/// The strong model where there is one, deliberately: the picker is one click
/// away for anyone who wants cheaper, where a weak default costs a turn that
/// has to be redone by hand. Mirrors `default_model_for` in `models/models.rs`
/// — two readers that cannot call each other, so the rule is stated twice.
///
/// pi names none, and that is the honest answer rather than a gap. It is
/// multi-provider, so any constant here might name a model the reader has no
/// key for — and pi's own settings already say which one they want. The
/// composer reads that back instead of seeding it.
export const DEFAULT_MODEL_FOR: Record<Harness, ModelId> = {
  claude_code: "opus",
  codex: "gpt56_sol",
  pi: UNSET_MODEL,
  // Multi-provider like pi, and its settings file already names a model.
  fx: UNSET_MODEL,
  // One vendor and a list this build can name, so the bare alias does what it
  // does for Claude Code: follow whatever xAI ship under it.
  grok: "grok-4.7",
  // Multi-provider like pi and fx: 388 models on a machine with two providers
  // logged in, so anything named here could be one the reader has no key for.
  opencode: UNSET_MODEL,
  // The same again: 308 models following whichever provider the reader signed
  // in to, and its own settings already name one.
  cline: UNSET_MODEL,
};

/// The providers `fx provider` takes, in fx's own words. Fixed by fx's CLI
/// (`fx provider <gateway|codex|grok>`), not discovered — the *models* are.
/// `label` is fx's own full name, and it is the screen reader's alone — the
/// control is marks, drawn by `ProviderIcon`.
export const FX_PROVIDERS: { id: string; label: string }[] = [
  { id: "gateway", label: "Vercel AI Gateway" },
  { id: "codex", label: "Codex subscription" },
  { id: "grok", label: "Grok subscription" },
];

/// Which model each harness was last left on. Absent key = never picked one.
export type ModelByHarness = Partial<Record<Harness, ModelId>>;

/// The model to open a harness on: what it was last left on, else its default.
///
/// Per-harness because a model belongs to exactly one of them, so a single
/// remembered pick can only ever be right for the harness that made it.
/// Switching to Codex and back used to land on whichever model the new list
/// happened to start with — a pick nobody made, and one that read as the
/// composer forgetting.
export function rememberedModel(remembered: ModelByHarness, harness: Harness): ModelId {
  return remembered[harness] ?? DEFAULT_MODEL_FOR[harness];
}

/// The model to run, given a pick and the list the current harness can run.
///
/// A model belongs to exactly one harness, so a pick made under the other one
/// names something this harness cannot run — and the pick is stored, so it
/// outlives the switch that made it. Every place that seeds the composer's
/// model has to ask this: repairing only where the harness *changes* leaves the
/// stored default free to name the old harness's model forever, and it reaches
/// the backend as a session started on a model nobody chose.
///
/// An empty list means the models have not arrived yet, so the pick stands —
/// the fetch that fills the list repairs it a beat later.
///
/// A pick it has to replace falls to the harness's default, not to whatever
/// leads the list: the head of the list is a picker-ordering decision, and
/// reading it as an answer is what put sessions on Fable and Sol.
export function usableModel(models: Model[], picked: ModelId, harness: Harness): ModelId {
  if (models.length === 0 || models.some((m) => m.id === picked)) return picked;

  const fallback = DEFAULT_MODEL_FOR[harness];

  // A harness naming no default answers the sentinel, never the head of the
  // list: pi picks for itself, and the spawn omits the flag. A pick this list
  // cannot run — the other harness's model, or one whose provider was logged
  // out — falls to "let pi decide", where landing on the list's first model
  // put a session on a model the reader never chose, with nothing on screen
  // saying so. Same answer for the unset pick.
  if (isUnsetModel(fallback)) return UNSET_MODEL;

  return models.some((m) => m.id === fallback) ? fallback : models[0].id;
}

/// fx's model repair, per provider. fx lists one provider at a time, so a pick
/// made under another provider names a model this list cannot run — and unlike
/// [`usableModel`], the fall-back is not the unset sentinel outright but the
/// model this provider was **last left on**, so switching providers and back
/// returns to where you were. Only when that too is gone does it fall to the
/// sentinel (fx picks for itself), never to the head of the list.
///
/// `picks` is the reader's last model per provider; the caller reads it, so this
/// stays pure and testable.
export function usableFxModel(
  list: Model[],
  picked: ModelId,
  picks: Record<string, ModelId>,
): ModelId {
  if (list.length === 0 || list.some((m) => m.id === picked)) return picked;
  const remembered = picks[list[0]?.provider ?? ""];
  if (remembered && list.some((m) => m.id === remembered)) return remembered;
  return UNSET_MODEL;
}

/// The provider serving this model, from the lists fx has answered so far, or
/// `undefined` where none of them names it. What lets a session's model say
/// which provider it belongs to without a field on the index for it.
export function fxProviderOf(cache: Record<string, Model[]>, id: ModelId): string | undefined {
  if (isUnsetModel(id)) return undefined;
  return Object.keys(cache).find((provider) => cache[provider].some((m) => m.id === id));
}

/// The fx list the composer draws: the one serving `picked`, where the cache
/// knows it, else `active` — the list fx's global provider last answered.
///
/// fx's provider is one setting for the whole machine, and the composer used to
/// draw its list from that alone — so switching provider in one session put the
/// new provider's thumb and rows under every other fx session's picker, drew
/// their models as bare ids, and let ⇧⇥ cycle them onto the wrong provider.
/// The pick is per session and names its provider, so the list follows it.
export function fxListFor(
  cache: Record<string, Model[]>,
  picked: ModelId,
  active: Model[],
): Model[] {
  const own = fxProviderOf(cache, picked);
  if (!own || own === active[0]?.provider) return active;
  return cache[own] ?? active;
}

/// The pick once fx's global list lands. Kept where the cache names its
/// provider — that is a session's own model, and the read may be landing after
/// the reader moved onto it from the session whose switch asked for it — else
/// repaired against the landed list as [`usableFxModel`] does.
export function landedFxModel(
  cache: Record<string, Model[]>,
  landed: Model[],
  current: ModelId,
  picks: Record<string, ModelId>,
): ModelId {
  return fxProviderOf(cache, current) ? current : usableFxModel(landed, current, picks);
}

/// The pick the moment a provider is switched to: repaired against that
/// provider's cached list, or with none cached its last pick — the pick still
/// has to leave the old provider, or [`fxListFor`] keeps drawing the old
/// provider's list and the switch reads as having done nothing.
export function seededFxModel(
  cache: Record<string, Model[]>,
  provider: string,
  current: ModelId,
  picks: Record<string, ModelId>,
): ModelId {
  const cached = cache[provider];
  if (!cached?.length) return picks[provider] ?? UNSET_MODEL;
  return usableFxModel(cached, current, picks);
}

/// The effort a model will actually run at, given what the reader last picked
/// for it.
///
/// A remembered pick outlives the answer that made it offerable, and fx is
/// where that bites: its ladder is per model and only a live session can state
/// it, so a level picked off the provider's guess can stop being on the list
/// the moment a session reports the truth (DRA-221). Left unchecked the trigger
/// names a level the menu beside it no longer offers, and the next send asks
/// for it again — which is the state the reader complained about in the first
/// place.
///
/// A model that takes no effort answers `null`, which is what hides the control
/// entirely. Otherwise the first offered level of: the pick, the model's own
/// default, the app's — [`usableModel`]'s own rule, that a pick which cannot be
/// honoured falls to a *default* rather than to whatever happens to sit nearest
/// it in the list. Only where none of the three is offered does the shape of
/// the ladder decide, and then it is the **top** rung: the app default is
/// already near the top, so a ladder missing it is a short one, and the top of
/// a short ladder is closer to what was asked than its floor.
export function usableEffort(
  model: Model,
  remembered: Effort | null,
  fallback: Effort,
): Effort | null {
  if (model.efforts.length === 0) return null;
  for (const wanted of [remembered, model.defaultEffort, fallback]) {
    if (wanted && model.efforts.includes(wanted)) return wanted;
  }
  return model.efforts[model.efforts.length - 1];
}

/// The agents the picker offers, in the order it draws them.
///
/// **A harness absent here still runs.** This list decides what the row *draws*;
/// `Harness` is whole in Rust and a session already recorded on any of them
/// spawns, resumes and streams as before. pi is the standing example — it is
/// drawn nowhere and is what the OpenRouter slot below spawns.
///
/// fx, grok and pi were all drawn once and are not any more: the row is one
/// pick a reader makes before they write a prompt, and four marks they use beat
/// seven they scroll past. Dropping one costs the *pick*, never the sessions.
///
/// **opencode is drawn as an agent of its own, and the `opencode-pi` bridge is
/// not drawn at all.** The bridge was the cheaper reading — one CLI to keep
/// signed in for a catalogue pi already serves — and it cannot work: opencode's
/// free tier answers **403 `FreeTierError`, "OpenCode's free tier can only be
/// used from within OpenCode"**, to any turn whose agent has its tools denied,
/// which is exactly what a model-proxy bridge is. Measured against opencode
/// 1.18.30 — the same model and the same flags succeed with tools on and fail
/// with them off — so no version of the bridge can serve those models, and the
/// only way to reach them is opencode running as itself.
export const HARNESS_ORDER: Harness[] = ["claude_code", "opencode", "cline", "codex"];

/// The provider pi serves OpenRouter's catalogue under, spelled as pi spells it.
export const OPENROUTER = "openrouter";

/// One mark in the picker's agent row: an agent, or an agent narrowed to one of
/// its providers.
///
/// A slot is not a harness. OpenRouter ships no CLI of its own — it is a
/// catalogue pi already serves, and a pi model id names its provider
/// (`openrouter/anthropic/claude-opus-5`) — so the slot spawns pi and narrows
/// the list, where a `Harness` variant of its own would be a lie on the index
/// and a second copy of every pi path in Rust.
export type AgentSlot = {
  id: string;
  harness: Harness;
  /// The provider the slot narrows to, `null` where it draws the agent whole.
  /// Compared against a model's own `provider` field, which is pi's spelling.
  provider: string | null;
};

/// The slots in the order the picker draws them, which is also the order ⌘⇧A
/// steps through. One list: a chord visiting a slot the row cannot show, or
/// skipping one it can, reads as the chord being broken.
///
/// OpenRouter sits at the end, since it is the one entry here that names a
/// catalogue instead of an agent — and it is the only reason pi is still
/// spawned at all, pi being drawn nowhere in [`HARNESS_ORDER`].
export const AGENT_SLOTS: AgentSlot[] = [
  ...HARNESS_ORDER.map((harness) => ({ id: harness as string, harness, provider: null })),
  { id: OPENROUTER, harness: "pi", provider: OPENROUTER },
];

/// Whether the row actually draws this pairing.
///
/// A stored preference outlives the list: somebody who last started a session on
/// fx, grok or pi keeps that harness in local storage after it stops being
/// drawn, and nothing about the picker would say so — the thumb falls back to
/// the first slot while the send still spawns the agent that is no longer
/// there. So the composer repairs its own pick against this rather than trusting
/// what was written.
export function isDrawnSlot(harness: Harness, provider: string | null): boolean {
  return AGENT_SLOTS.some((s) => s.harness === harness && s.provider === provider);
}

/// The slot a harness and provider name, falling to the harness's own whole
/// slot — a provider no slot draws is a narrowing nothing in the row can show,
/// so the thumb belongs under the agent itself.
export function slotOf(harness: Harness, provider: string | null): AgentSlot {
  return (
    AGENT_SLOTS.find((s) => s.harness === harness && s.provider === provider) ??
    AGENT_SLOTS.find((s) => s.harness === harness && s.provider === null) ??
    AGENT_SLOTS[0]
  );
}

/// Where ⌘⇧A lands from here, wrapping. An unknown slot steps onto the first,
/// the same place the picker parks its thumb.
export function nextSlot(harness: Harness, provider: string | null): AgentSlot {
  // `findIndex`, not `slotOf`: an unrecognised pair answers -1 and parks on the
  // first slot, where stepping from the slot `slotOf` fell back to would skip it.
  const i = AGENT_SLOTS.findIndex((s) => s.harness === harness && s.provider === provider);
  return AGENT_SLOTS[(i + 1) % AGENT_SLOTS.length];
}

/// The slot provider a pick belongs to, or `null` where no slot draws one.
///
/// Derived rather than stored, so a session opened from the sidebar lands on
/// the slot its own model names: a pi id is `provider/model`
/// (`openrouter/anthropic/claude-opus-5`), and the provider is the part before
/// the first slash. A provider with no slot of its own answers `null`, which is
/// the agent's whole list — the same answer as a harness that has no slots.
export function slotProviderOf(harness: Harness, id: ModelId): string | null {
  const opens = String(id).split("/")[0];
  const slot = AGENT_SLOTS.find((s) => s.harness === harness && s.provider === opens);
  return slot?.provider ?? null;
}

/// The list a slot draws: every model where it names no provider, that
/// provider's alone where it does.
export function slotModels(models: Model[], provider: string | null): Model[] {
  return provider ? models.filter((m) => m.provider === provider) : models;
}

/// The pick once a slot's list has landed, narrowed to the slot's provider.
///
/// [`usableModel`]'s rule with one addition: a pick this provider does not
/// serve is *not* the provider's business to keep, however runnable the harness
/// finds it. A slot that narrows the list has to narrow the pick with it, or
/// the trigger names a model no row in the menu offers — and the send runs it.
/// `starred` is the reader's own order, so the fall-back is a model they chose
/// rather than whichever row leads the list; nothing starred falls to the unset
/// sentinel, which is pi picking for itself.
export function usableSlotModel(
  models: Model[],
  picked: ModelId,
  harness: Harness,
  provider: string | null,
  starred: ModelId[],
): ModelId {
  if (!provider) return usableModel(models, picked, harness);

  const list = slotModels(models, provider);
  if (list.length === 0 || list.some((m) => m.id === picked)) return picked;

  const first = starred.find((id) => list.some((m) => m.id === id));
  return first ?? UNSET_MODEL;
}
