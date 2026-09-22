import { useEffect, type ReactNode } from "react";
import { ChevronLeft, PanelLeft } from "lucide-react";

import { hasOpenOverlay, useBackLayer } from "@/lib/backStack";
import { DROP_ATTR } from "@/lib/dragSession";
import { EMPTY_VIEW } from "@/lib/groups";
import { setPhoneDrawer, useIsNarrow, usePhoneDrawer } from "@/lib/phoneLayout";
import { cn } from "@/lib/utils";

/// How far a touch must travel sideways before it is a swipe rather than a tap
/// that moved. Deliberately short: the gesture is how both side panes are
/// reached on a phone, and the screen edges are the system's — Android's own
/// back gesture eats them — so this has to answer a swipe made anywhere,
/// including one made in a hurry across the middle of the transcript.
const SWIPE = 40;

/// How much more sideways than vertical it has to be. A drag while reading is a
/// scroll, and this is what keeps one from opening a pane; low enough that a
/// swipe made with the thumb, which arcs, still counts.
const SIDEWAYS = 1.2;

/// Whether the touch began somewhere that scrolls sideways for itself — a code
/// block, a diff, the phone's own tab strip. Those keep their scrolling: a
/// gesture that stole it would make the content unreadable, and they are the
/// one place a horizontal drag already means something.
function scrollsSideways(target: EventTarget | null): boolean {
  let el = target instanceof Element ? target : null;

  while (el && el !== document.body) {
    if (el.scrollWidth - el.clientWidth > 8) {
      const overflow = getComputedStyle(el).overflowX;
      if (overflow === "auto" || overflow === "scroll") return true;
    }
    el = el.parentElement;
  }

  return false;
}

type AppShellProps = {
  sidebar: ReactNode;
  header?: ReactNode;
  footer: ReactNode;
  /// Right-hand inspector. Sits outside the chat column so the composer stays
  /// scoped to the conversation rather than spanning both.
  panel?: ReactNode;
  /// Put the inspector away. Narrow only, where it takes the whole window and
  /// the reader would otherwise have no way back to the conversation.
  onPanelClose?: () => void;
  /// Bring it out. Narrow only, and only the swipe uses it — the tab row is
  /// how it is opened by hand.
  onPanelOpen?: () => void;
  /// Drawn under the header on a narrow window and nowhere else. The tab row
  /// lives here rather than in the header, which cannot hold a session's name
  /// and four labels at a phone's width.
  subheader?: ReactNode;
  /// Whether that inspector is actually drawing anything. The node is always
  /// handed over and decides for itself, so its truthiness says nothing — and a
  /// narrow layout, which draws it over the whole window, has to know the
  /// difference or it covers the app with an empty pane.
  panelOpen?: boolean;
  /// The crew — the sessions this conversation started — when it has any. Inside
  /// the main column so it comes and goes with the view tabs, outside the chat
  /// column for the panel's reason: a composer spanning a list of other
  /// conversations does not say which one it talks to.
  crew?: ReactNode;
  /// Holds the composer in the upper middle of the window and drops the
  /// transcript pane. The empty state has no transcript to anchor the composer
  /// against, so pinning it to the bottom leaves the one usable control as far
  /// from the eye as the window allows. `children` is not rendered in this state.
  centered?: boolean;
  /// Drawn over the centered column — the drop zone, since a sidebar row let
  /// go on the empty state opens the session whole and the zone has to say so
  /// where the pointer is.
  overlay?: ReactNode;
  children: ReactNode;
};

/// Owns every bit of app geometry: the viewport lock, which panes scroll, and where
/// the composer sits. Panes below this get height from their parent and never set
/// their own margins, so there is one place to change the layout.
export default function AppShell({
  sidebar,
  header,
  footer,
  panel,
  panelOpen = false,
  onPanelClose,
  onPanelOpen,
  subheader,
  crew,
  centered = false,
  overlay,
  children,
}: AppShellProps) {
  const narrow = useIsNarrow();
  const drawer = usePhoneDrawer();

  // What Android's Back takes away here, innermost first. Registered in this
  // order so the inspector — which covers the whole window, drawer included —
  // is the one a press closes while both are open.
  useBackLayer(narrow && drawer, () => setPhoneDrawer(false));
  useBackLayer(narrow && panelOpen, () => onPanelClose?.());

  // Both side panes are reached by swiping, which on a phone is the only
  // gesture there is room for. **Anywhere on the screen, not from an edge**:
  // Android's own back gesture owns both edges, so an edge-started swipe is one
  // the app never sees — and a reader holding the phone one-handed swipes
  // across the middle of the transcript, which is where the thumb is.
  //
  // Right brings the sessions out, left brings the inspector out, and a swipe
  // back over an open pane closes it. **Acted on at the release, never during
  // the drag**, so the listeners stay passive and nothing here fights the
  // page's own scrolling.
  useEffect(() => {
    if (!narrow) return;

    let startX = 0;
    let startY = 0;
    let tracking = false;

    const start = (e: TouchEvent) => {
      tracking = e.touches.length === 1 && !scrollsSideways(e.target) && !hasOpenOverlay();
      if (!tracking) return;

      const touch = e.touches[0];
      startX = touch.clientX;
      startY = touch.clientY;
    };

    const cancel = () => {
      tracking = false;
    };

    const end = (e: TouchEvent) => {
      if (!tracking) return;
      tracking = false;

      const touch = e.changedTouches[0];
      if (!touch) return;

      const dx = touch.clientX - startX;
      const dy = touch.clientY - startY;
      if (Math.abs(dx) < SWIPE || Math.abs(dx) < Math.abs(dy) * SIDEWAYS) return;

      if (dx > 0) {
        if (drawer) return;
        if (panelOpen) return onPanelClose?.();
        return setPhoneDrawer(true);
      }

      if (drawer) return setPhoneDrawer(false);
      if (panelOpen) return;
      onPanelOpen?.();
    };

    document.addEventListener("touchstart", start, { passive: true });
    document.addEventListener("touchend", end, { passive: true });
    document.addEventListener("touchcancel", cancel, { passive: true });
    return () => {
      document.removeEventListener("touchstart", start);
      document.removeEventListener("touchend", end);
      document.removeEventListener("touchcancel", cancel);
    };
  }, [narrow, drawer, panelOpen, onPanelClose, onPanelOpen]);

  return (
    <div className="flex h-full w-full overflow-hidden">
      {/* Narrow, the sidebar is drawn over the chat rather than beside it, and
          the panel takes the window whole. Neither is unmounted: a sidebar that
          comes back rebuilt loses its scroll and its open groups, and the panel
          holds reads it would have to make again. */}
      {narrow ? (
        <>
          {drawer && (
            <button
              className="fixed inset-0 z-30 bg-black/40"
              aria-label="Close sessions"
              onClick={() => setPhoneDrawer(false)}
            />
          )}
          <div
            data-phone-drawer=""
            className={cn(
              // Drawn over the chat, so it needs a fill of its own. In the flow it
              // deliberately has none and reads straight through to the body's
              // gradient; over the transcript that makes both unreadable.
              "bg-background fixed inset-y-0 left-0 z-40 flex border-r pt-[env(safe-area-inset-top)] pb-[env(safe-area-inset-bottom)] shadow-xl transition-transform duration-200",
              !drawer && "-translate-x-full",
            )}
          >
            {sidebar}
          </div>
        </>
      ) : (
        sidebar
      )}

      {/* `min-w-0` is load-bearing: without it a wide code block in the transcript
          sets the flex item's floor and pushes the sidebar off-screen. */}
      <div className="flex min-w-0 flex-1 flex-col">
        {narrow ? (
          <div className="flex min-w-0 items-center">
            <button
              className="text-muted-foreground hover:text-foreground shrink-0 px-3 py-2"
              aria-label="Sessions"
              onClick={() => setPhoneDrawer(true)}
            >
              <PanelLeft className="size-4" />
            </button>
            <div className="min-w-0 flex-1">{header}</div>
          </div>
        ) : (
          header
        )}
        {/* Scrolls rather than squashing: four labels plus whatever a later
            view adds will outgrow a phone's width, and a row that shrinks its
            own labels is harder to read than one you swipe. */}
        {narrow && subheader && (
          <div className="flex shrink-0 items-center overflow-x-auto border-b px-2 py-1">
            {subheader}
          </div>
        )}
        {centered ? (
          // Held below the top rather than centered. Centering moves the whole
          // block every time the textarea grows a line, so the wordmark and the
          // toolbar drift upwards as you type; a fixed offset keeps everything
          // above the input still and lets the box grow downwards alone. The
          // offset is a proportion of the window rather than a fixed inset, which
          // would read as top-aligned on a tall window and off-centre on a short
          // one. `overflow-y-auto` only ever engages on a window too short to
          // hold the composer at its full height — the box caps itself well
          // before that at the default size.
          <div
            className="relative flex min-h-0 flex-1 flex-col items-center overflow-y-auto"
            {...{ [DROP_ATTR]: EMPTY_VIEW }}
          >
            {/* `children` is deliberately dropped: there is no transcript to
                show, and the composer is the whole state. */}
            <div className={cn("w-full shrink-0", narrow ? "pt-6" : "pt-[13vh]")}>{footer}</div>
            {overlay}
          </div>
        ) : (
          // The crew runs the full height beside both, so its rows get the
          // composer's band too rather than stopping short of it at a line
          // nothing else on screen is drawn to.
          <div className="flex min-h-0 flex-1">
            <div className="flex min-w-0 flex-1 flex-col">
              {/* `flex flex-col` so the view tabs' bodies, which size themselves
                  with `flex-1` the way the right panel's do, have a column to
                  grow in. */}
              <div className="flex min-h-0 flex-1 flex-col overflow-hidden">{children}</div>
              <div className="shrink-0">{footer}</div>
            </div>
            {/* A fixed 320px column has nowhere to stand beside a phone's
                transcript, so the crew is a wide-window arrangement only. */}
            {narrow ? null : crew}
          </div>
        )}
      </div>

      {narrow && panelOpen ? (
        <div
          data-phone-panel=""
          className="bg-background fixed inset-0 z-40 flex flex-col pt-[env(safe-area-inset-top)] pb-[env(safe-area-inset-bottom)]"
        >
          {/* The pane fills the window here, so the only way back to the
              conversation has to be drawn. */}
          <button
            className="text-muted-foreground hover:text-foreground flex h-11 shrink-0 items-center gap-1 px-3 text-ui"
            onClick={onPanelClose}
          >
            <ChevronLeft className="size-4" />
            Back
          </button>
          <div className="flex min-h-0 flex-1">{panel}</div>
        </div>
      ) : (
        panel
      )}
    </div>
  );
}
