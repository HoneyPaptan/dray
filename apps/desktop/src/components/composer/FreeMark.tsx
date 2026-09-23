import type { Model } from "@/types/events";

/// The mark a row wears when the model costs nothing to run.
///
/// One word in the muted colour, drawn exactly like the effort and Fast
/// qualifiers beside it: its *presence* is the whole message, so an accent
/// would be a second way to say one thing — and the yellow this palette spends
/// on sessions wanting the reader is not free to mean "cheap" as well.
///
/// Stated once rather than per row shape, since the picker draws a model row
/// three ways and a mark appearing on only two of them reads as the other
/// model not being free.
export function FreeMark({ model }: { model: Model }) {
  if (!model.free) return null;
  return <span className="shrink-0 text-muted-foreground/60">Free</span>;
}
