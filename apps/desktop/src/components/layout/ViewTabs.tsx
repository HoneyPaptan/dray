import ShortcutKeys from "@/components/ShortcutKeys";
import TabButton from "@/components/TabButton";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { IS_MAC } from "@/lib/platform";
import type { ShortcutId } from "@/lib/shortcuts";

/// Which view fills the main column. Set and order in one — a Terminal view
/// joins by being added here and given a body.
export const VIEW_TABS = ["chat", "browser", "changes", "files"] as const;

export type ViewTab = (typeof VIEW_TABS)[number];

/// Whether this build has a browser behind that tab.
///
/// CEF is a macOS-only dependency here, so off macOS the tab's commands are not
/// registered at all and pressing it rejects with "command not found" — a tab
/// that looks like every other one and answers nothing.
///
/// **Hidden rather than drawn as an empty state, which reverses what the PR
/// panel does one screen over.** That pane keeps its tab and explains itself
/// because one `brew install` is the cure; here there is no cure to name, and a
/// pane whose whole content is "not in this build" is a control that exists to
/// say it does not work.
///
/// `IS_MAC` and not a question asked of the backend: a macOS `pnpm tauri dev`
/// has no `cef` feature either, so the honest test is whether the commands are
/// registered — and answering that needs a round trip before the first paint,
/// which would flicker the row on every launch. This keeps macOS exactly as it
/// was and takes the tab off the platform that can never have it.
export const HAS_BROWSER = IS_MAC;

/// The tabs actually drawn, in order.
const DRAWN_TABS = VIEW_TABS.filter((tab) => tab !== "browser" || HAS_BROWSER);

const LABELS: Record<ViewTab, string> = {
  chat: "Chat",
  changes: "Diff",
  browser: "Browser",
  files: "Files",
};

/// Per tab rather than by position, since a rebinding names the view and not
/// its slot in the row.
const VIEW_SHORTCUTS: Record<ViewTab, ShortcutId> = {
  chat: "view.chat",
  changes: "view.changes",
  browser: "view.browser",
  files: "view.files",
};

/// The main column's tab row, drawn in the titlebar beside the session's name.
///
/// Styled as the right panel's tab row rather than as buttons, because they are
/// the same control: one row where exactly one entry is on.
export default function ViewTabs({
  tab,
  onChange,
}: {
  tab: ViewTab;
  onChange: (tab: ViewTab) => void;
}) {
  return (
    // Shrinks and clips rather than holding its width: the session name beside
    // it gives up first, but past that four fixed labels are still wider than a
    // narrow column, and a row that refuses to shrink pushes the panel toggle
    // out of the column and draws it over the pane next door.
    <div className="flex min-w-0 items-center gap-0.5 overflow-hidden">
      {DRAWN_TABS.map((value) => (
        <Tooltip key={value}>
          <TooltipTrigger asChild>
            {/* No fill on the selected one, unlike the other two rows this
                button serves. Those sit *inside* a panel they are the tabs of,
                where a fill reads as the pane's own top edge; this row sits in
                the titlebar beside the session's name, over the window's glass
                and next to nothing, so the fill was a lozenge floating in the
                chrome — the loudest shape in a strip whose whole job is to be
                quiet. Colour alone marks it, the same currency the sidebar
                spends on the selected session. */}
            {/* And tighter with it. `px-2` was sized for the fill — it is the
                lozenge's own inset, and with the lozenge gone it is a gap
                between words that reads as four separate controls rather than
                as one row. What the padding still buys is the click target, so
                it comes down a rung rather than off. */}
            <TabButton
              active={tab === value}
              className="bg-transparent px-1.5"
              onClick={() => onChange(value)}
            >
              {LABELS[value]}
            </TabButton>
          </TooltipTrigger>
          {/* The name is already on the button, so the tooltip carries the
              keycaps alone rather than repeating it back — and with nothing to
              read, the default `px-3` is a margin around one small chip. The
              base style already tightens the *right* side for a trailing
              keycap; this matches the left to it. */}
          <TooltipContent side="bottom" className="px-1.5">
            <ShortcutKeys ids={[VIEW_SHORTCUTS[value]]} />
          </TooltipContent>
        </Tooltip>
      ))}
    </div>
  );
}
