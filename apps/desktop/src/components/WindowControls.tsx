import { getCurrentWindow } from "@tauri-apps/api/window";
import { Minus, Square, X } from "lucide-react";

import { OWN_WINDOW_CONTROLS } from "@/lib/platform";
import { cn } from "@/lib/utils";

/// Minimise, maximise and close, for the platforms macOS does not draw them on.
///
/// Fixed to the window rather than placed in a titlebar row, and that is the
/// whole reason it is a component of its own: which row reaches the window's
/// right edge changes with the right panel, so a row that owned these buttons
/// would have to hand them over every time the pane opened. Fixed, the buttons
/// stay put and the rows underneath only reserve the width — `--window-controls-w`,
/// the mirror of `--traffic-lights-w` on the other side.
///
/// Renders nothing on macOS, where the traffic lights are real.
export function WindowControls() {
  if (!OWN_WINDOW_CONTROLS) return null;

  const win = getCurrentWindow();

  return (
    // `fixed` and not `absolute`: the shell's panes each establish their own
    // containing block, and the window's corner belongs to none of them.
    //
    // No drag region. Tauri stops walking up at any `BUTTON`, so the row behind
    // these keeps dragging and the buttons keep clicking without either opting
    // out of the other.
    <div className="fixed top-0 right-0 z-50 flex h-(--titlebar-h) items-center gap-0.5 pr-2">
      <ControlButton label="Minimise" onPress={() => void win.minimize()}>
        <Minus className="size-3.5" />
      </ControlButton>
      <ControlButton label="Maximise" onPress={() => void win.toggleMaximize()}>
        <Square className="size-3" />
      </ControlButton>
      {/* Goes through the window's close request rather than exiting, so the
          quit confirmation the menu bar raises on macOS is raised here too. */}
      <ControlButton label="Close" destructive onPress={() => void win.close()}>
        <X className="size-3.5" />
      </ControlButton>
    </div>
  );
}

function ControlButton({
  label,
  destructive = false,
  onPress,
  children,
}: {
  label: string;
  destructive?: boolean;
  onPress: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      onClick={onPress}
      className={cn(
        "text-muted-foreground flex size-7 items-center justify-center rounded-md transition-colors",
        destructive
          ? "hover:bg-destructive hover:text-destructive-foreground"
          : "hover:bg-muted hover:text-foreground",
      )}
    >
      {children}
    </button>
  );
}
