/// The height the app may actually draw in, published as `--viewport-h`.
///
/// A phone's soft keyboard is the whole reason this exists. `100dvh` tracks the
/// browser's own retracting chrome and says nothing about the keyboard, so the
/// app stayed full height with the keyboard over its lower third — and the
/// webview answered by *panning* the whole page up to keep the caret in sight,
/// which took the header off the top of the screen. `interactive-widget` in the
/// viewport meta is the declarative cure and is not honoured under Android's
/// edge-to-edge window, so the measurement has to be made here.
///
/// `visualViewport.height` is what the reader can see. Writing it as a custom
/// property rather than a class keeps the layout rules in CSS beside the rest of
/// the narrow ones, and the desktop is unaffected: there the value equals the
/// window's own height and the rule that reads it is inside the narrow query.
export function watchViewport() {
  const vv = typeof window === "undefined" ? null : window.visualViewport;
  if (!vv) return;

  const apply = () => {
    const root = document.documentElement;
    root.style.setProperty("--viewport-h", `${Math.round(vv.height)}px`);
    // What the keyboard took, for anything that needs to clear it directly.
    root.style.setProperty(
      "--keyboard-h",
      `${Math.max(0, Math.round(window.innerHeight - vv.height - vv.offsetTop))}px`,
    );
    // Undo any pan the webview made before this landed. Harmless where it made
    // none — the app owns every scroll container, so the document itself is
    // never meant to be scrolled at all.
    if (window.scrollY !== 0) window.scrollTo(0, 0);
  };

  apply();
  vv.addEventListener("resize", apply);
  vv.addEventListener("scroll", apply);
}
