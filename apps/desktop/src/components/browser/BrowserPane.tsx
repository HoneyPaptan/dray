import { openUrl } from "@tauri-apps/plugin-opener";
import {
  ArrowLeft,
  ArrowRight,
  ExternalLink,
  Globe,
  Keyboard,
  Maximize2,
  Minimize2,
  Plus,
  RotateCcw,
  RotateCw,
  Smartphone,
  X,
} from "lucide-react";
import { useEffect, useId, useRef, useState } from "react";

import { Button } from "@/components/ui/button";
import Spinner from "@/components/ui/spinner";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import {
  activateTab,
  claimStage,
  closeTab,
  diffInput,
  listLocalServers,
  modifiersOf,
  navigate,
  normalizeUrl,
  openInBrowser,
  pagePoint,
  releaseStage,
  sendInput,
  setPendingTab,
  setViewport,
  useBrowserTabs,
  useFrame,
  useOpenError,
  usePendingTab,
  useViewport,
  VIEWPORT_PRESETS,
  type BrowserTab,
  type LocalServer,
  type Viewport,
} from "@/lib/browser";
import { cn } from "@/lib/utils";

/// A touch screen is what this is drawn on. Decides two things: the page is
/// laid out as a phone and driven with touch events, and the toolbar offers
/// a way to bring the soft keyboard up, since a tap on a field inside a
/// picture of a page raises nothing.
const TOUCH = typeof navigator !== "undefined" && navigator.maxTouchPoints > 0;

/// The toolbar sits on the card surface, a step off the ghost hover fill at
/// best, so a hovered button there was invisible. A wash of the foreground
/// instead.
const TOOL_BTN = "hover:bg-foreground/10 aria-expanded:bg-foreground/10";

const TIP_SIDE = "top" as const;

/// The session's browser: tab strip, URL bar, navigation, and the page.
///
/// Two mounts of one component. The right panel's Browser tab is where it
/// lives; the main column's is the full view, reached by the expand button,
/// and while that is open the panel says so instead of drawing a second copy.
///
/// The page is a screencast of a headless Chromium on the machine running the
/// runtime, drawn as an image the pane sizes; pointer and key events on that
/// image go back as CDP input. Same picture on the desktop and on a phone.
export default function BrowserPane({
  sessionId,
  active,
  mode,
  fullOpen = false,
  onExpand,
  onCollapse,
}: {
  sessionId: string;
  active: boolean;
  mode: "panel" | "full";
  /// The full view is showing, so the panel mount stands aside.
  fullOpen?: boolean;
  onExpand?: () => void;
  onCollapse?: () => void;
}) {
  const tabs = useBrowserTabs(sessionId) ?? [];
  const pending = usePendingTab(sessionId);
  const current = pending ? null : (tabs.find((t) => t.active) ?? null);
  const viewport = useViewport(sessionId);
  const [deviceBar, setDeviceBar] = useState(false);
  const [typing, setTyping] = useState(false);
  const standingAside = mode === "panel" && fullOpen;
  const empty = !current;

  if (standingAside) {
    return (
      <div className="flex min-h-0 flex-1 flex-col items-center justify-center gap-3 p-8 text-center text-ui text-muted-foreground">
        <span>Open in the full view.</span>
        {onCollapse && (
          <Button variant="outline" size="sm" onClick={onCollapse}>
            Bring it back here
          </Button>
        )}
      </div>
    );
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <Chrome
        sessionId={sessionId}
        tabs={tabs}
        current={current}
        pending={pending}
        mode={mode}
        deviceBar={deviceBar}
        onToggleDeviceBar={() => setDeviceBar((v) => !v)}
        typing={typing}
        onToggleTyping={() => setTyping((v) => !v)}
        onExpand={onExpand}
        onCollapse={onCollapse}
      />
      {deviceBar && (
        <DeviceBar sessionId={sessionId} viewport={viewport} onClose={() => setDeviceBar(false)} />
      )}
      {typing && !empty && <TypingStrip sessionId={sessionId} onClose={() => setTyping(false)} />}
      {empty ? (
        <div className="relative min-h-0 flex-1 bg-background">
          <Servers sessionId={sessionId} />
        </div>
      ) : (
        <Stage sessionId={sessionId} active={active} viewport={viewport} />
      )}
    </div>
  );
}

/// Where the page is drawn, and where the reader's hands land on it.
///
/// Responsive, the page is laid out at the stage's own size and the frame
/// fills it; with a device preset the page is laid out at that size and drawn
/// centred, scrolling where it is taller than the stage. Either way the frame
/// is drawn at the size the page was laid out at, so a pointer maps by the
/// drawn box alone.
function Stage({
  sessionId,
  active,
  viewport,
}: {
  sessionId: string;
  active: boolean;
  viewport: Viewport | null;
}) {
  const key = useId();
  const frame = useFrame(sessionId);
  const stageRef = useRef<HTMLDivElement>(null);
  const [box, setBox] = useState<{ width: number; height: number } | null>(null);

  useEffect(() => {
    const stage = stageRef.current;
    if (!stage) return;
    const measure = () => {
      const r = stage.getBoundingClientRect();
      const next = { width: Math.round(r.width), height: Math.round(r.height) };
      if (next.width > 0 && next.height > 0) {
        setBox((was) => (was?.width === next.width && was?.height === next.height ? was : next));
      }
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(stage);
    return () => observer.disconnect();
  }, []);

  const wanted = viewport ?? box;
  const width = wanted?.width ?? 0;
  const height = wanted?.height ?? 0;

  useEffect(() => {
    if (!active || !width || !height) {
      releaseStage(key);
      return;
    }
    // Two device pixels per CSS pixel at most: a retina desktop's third
    // buys nothing legible and triples what every frame weighs on the wire.
    const scale = Math.min(window.devicePixelRatio || 1, 2);
    claimStage(key, sessionId, { width, height, scale, touch: TOUCH });
    return () => releaseStage(key);
  }, [key, sessionId, active, width, height]);

  return (
    <div
      ref={stageRef}
      className={cn(
        "relative min-h-0 flex-1 bg-background",
        viewport
          ? "flex items-start justify-center overflow-auto bg-surface-raised p-3"
          : "overflow-hidden",
      )}
    >
      {wanted && (
        <Page
          sessionId={sessionId}
          src={frame?.src ?? null}
          width={width}
          height={height}
          framed={!!viewport}
        />
      )}
    </div>
  );
}

const BUTTONS = ["left", "middle", "right"] as const;

/// The frame, and every event on it turned into CDP input.
function Page({
  sessionId,
  src,
  width,
  height,
  framed,
}: {
  sessionId: string;
  src: string | null;
  width: number;
  height: number;
  framed: boolean;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const pressed = useRef<number | null>(null);
  const moveQueued = useRef(false);

  const at = (e: { clientX: number; clientY: number }) => {
    const box = ref.current?.getBoundingClientRect();
    if (!box) return { x: 0, y: 0 };
    return pagePoint({ x: e.clientX, y: e.clientY }, box, { width, height });
  };

  const mouse = (type: string, e: React.PointerEvent, button: number) => {
    const { x, y } = at(e);
    sendInput(sessionId, "Input.dispatchMouseEvent", {
      type,
      x,
      y,
      button: BUTTONS[button] ?? "none",
      buttons: e.buttons,
      clickCount: type === "mouseMoved" ? 0 : Math.max(1, e.detail),
      modifiers: modifiersOf(e),
    });
  };

  const touch = (type: string, e: React.PointerEvent) => {
    const points = type === "touchEnd" ? [] : [at(e)];
    sendInput(sessionId, "Input.dispatchTouchEvent", { type, touchPoints: points, modifiers: modifiersOf(e) });
  };

  // React registers `wheel` passively, so the scroll it carries would reach
  // the stage as well as the page; a listener of our own can refuse it.
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      const unit = e.deltaMode === 1 ? 20 : e.deltaMode === 2 ? height : 1;
      const { x, y } = at(e);
      sendInput(sessionId, "Input.dispatchMouseEvent", {
        type: "mouseWheel",
        x,
        y,
        deltaX: e.deltaX * unit,
        deltaY: e.deltaY * unit,
        modifiers: modifiersOf(e),
      });
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  }, [sessionId, width, height]);

  const key = (e: React.KeyboardEvent, down: boolean) => {
    // The app's own chords, on either accelerator. They reach `document`
    // whatever happens here, so sending them on as well would fire twice.
    if (e.metaKey || e.ctrlKey) return;
    e.preventDefault();
    sendKey(sessionId, e, down);
  };

  return (
    <div
      ref={ref}
      tabIndex={0}
      role="application"
      aria-label="Page"
      className={cn(
        "relative shrink-0 cursor-default select-none outline-none",
        framed && "rounded-sm shadow-[0_0_0_1px_var(--border)]",
      )}
      style={{ width, height, touchAction: "none" }}
      onPointerDown={(e) => {
        e.currentTarget.focus();
        e.currentTarget.setPointerCapture(e.pointerId);
        pressed.current = e.button;
        if (e.pointerType === "touch") touch("touchStart", e);
        else mouse("mousePressed", e, e.button);
      }}
      onPointerMove={(e) => {
        if (e.pointerType === "touch") {
          if (pressed.current !== null) touch("touchMove", e);
          return;
        }
        // One move a frame: the pointer reports far faster than a frame
        // can come back, and every one is a round trip.
        if (moveQueued.current) return;
        moveQueued.current = true;
        const { clientX, clientY, buttons, altKey, ctrlKey, metaKey, shiftKey } = e;
        requestAnimationFrame(() => {
          moveQueued.current = false;
          const { x, y } = at({ clientX, clientY });
          sendInput(sessionId, "Input.dispatchMouseEvent", {
            type: "mouseMoved",
            x,
            y,
            button: pressed.current === null ? "none" : (BUTTONS[pressed.current] ?? "none"),
            buttons,
            modifiers: modifiersOf({ altKey, ctrlKey, metaKey, shiftKey }),
          });
        });
      }}
      onPointerUp={(e) => {
        const button = pressed.current ?? e.button;
        pressed.current = null;
        if (e.pointerType === "touch") touch("touchEnd", e);
        else mouse("mouseReleased", e, button);
      }}
      onPointerCancel={(e) => {
        pressed.current = null;
        if (e.pointerType === "touch") touch("touchCancel", e);
      }}
      onContextMenu={(e) => e.preventDefault()}
      onKeyDown={(e) => key(e, true)}
      onKeyUp={(e) => key(e, false)}
    >
      {src ? (
        <img src={src} alt="" draggable={false} className="absolute inset-0 h-full w-full" />
      ) : (
        <div className="absolute inset-0 flex items-center justify-center text-muted-foreground">
          <Spinner className="size-4" />
        </div>
      )}
    </div>
  );
}

/// One key, as CDP wants it. A printable key carries its text on the way
/// down, which is what types it; Enter carries a return so a form submits;
/// anything else is a raw key the page reads by code.
function sendKey(sessionId: string, e: { key: string; code: string; keyCode: number; repeat: boolean } & Parameters<typeof modifiersOf>[0], down: boolean) {
  const printable = e.key.length === 1;
  const text = printable ? e.key : e.key === "Enter" ? "\r" : undefined;
  sendInput(sessionId, "Input.dispatchKeyEvent", {
    type: down ? (text ? "keyDown" : "rawKeyDown") : "keyUp",
    key: e.key,
    code: e.code,
    windowsVirtualKeyCode: e.keyCode,
    nativeVirtualKeyCode: e.keyCode,
    autoRepeat: e.repeat,
    modifiers: modifiersOf(e),
    ...(down && text ? { text, unmodifiedText: text } : {}),
  });
}

/// A text field of the pane's own, for a screen whose keyboard only comes up
/// for a field it can see. What is typed here is typed into the page: the
/// field's text is diffed on every change, since an IME rewrites a whole
/// word as it corrects it, and Enter and Backspace on an empty field go
/// through as keys.
function TypingStrip({ sessionId, onClose }: { sessionId: string; onClose: () => void }) {
  const last = useRef("");
  const press = (key: string, code: string, keyCode: number) => {
    const e = { key, code, keyCode, repeat: false, altKey: false, ctrlKey: false, metaKey: false, shiftKey: false };
    sendKey(sessionId, e, true);
    sendKey(sessionId, e, false);
  };
  return (
    <div className="flex h-9 shrink-0 items-center gap-1.5 border-b border-border bg-card px-2">
      <input
        autoFocus
        aria-label="Type into the page"
        placeholder="Type into the page"
        autoCapitalize="off"
        autoCorrect="off"
        spellCheck={false}
        className="h-7 min-w-0 flex-1 rounded-md bg-surface-raised px-2.5 text-ui outline-none focus:ring-1 focus:ring-ring dark:bg-background"
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            press("Enter", "Enter", 13);
            last.current = "";
            e.currentTarget.value = "";
          } else if (e.key === "Backspace" && e.currentTarget.value === "") {
            press("Backspace", "Backspace", 8);
          } else if (e.key === "Tab") {
            e.preventDefault();
            press("Tab", "Tab", 9);
          }
        }}
        onInput={(e) => {
          const next = e.currentTarget.value;
          const { del, add } = diffInput(last.current, next);
          for (let i = 0; i < del; i++) press("Backspace", "Backspace", 8);
          if (add) sendInput(sessionId, "Input.insertText", { text: add });
          last.current = next;
        }}
      />
      <Button variant="ghost" size="icon-sm" className={TOOL_BTN} aria-label="Hide keyboard field" onClick={onClose}>
        <X className="size-3.5" />
      </Button>
    </div>
  );
}

/// Tab strip and URL bar, coloured the way Chrome does it: the strip takes
/// the panel's own surface, the active tab and toolbar share the card, and
/// the URL field is cut into it, so which tab the bar belongs to is read
/// from colour alone.
function Chrome({
  sessionId,
  tabs,
  current,
  pending,
  mode,
  deviceBar,
  onToggleDeviceBar,
  typing,
  onToggleTyping,
  onExpand,
  onCollapse,
}: {
  sessionId: string;
  tabs: BrowserTab[];
  current: BrowserTab | null;
  pending: boolean;
  mode: "panel" | "full";
  deviceBar: boolean;
  onToggleDeviceBar: () => void;
  typing: boolean;
  onToggleTyping: () => void;
  onExpand?: () => void;
  onCollapse?: () => void;
}) {
  const [draft, setDraft] = useState<string | null>(null);
  const openError = useOpenError(sessionId);
  const inputRef = useRef<HTMLInputElement>(null);
  const url = current?.url ?? "";

  useEffect(() => {
    if (pending) inputRef.current?.focus();
  }, [pending]);

  const open = (raw: string, newTab: boolean) => {
    const target = normalizeUrl(raw);
    if (!target) return;
    void openInBrowser(sessionId, target, newTab).catch(() => undefined);
  };

  const newTab = () => {
    setPendingTab(sessionId, true);
    setDraft("");
  };

  const swap = mode === "panel" ? onExpand : onCollapse;

  return (
    <div className="shrink-0 border-b border-border">
      {(tabs.length > 0 || pending) && (
        <div className="flex h-8 items-end gap-1 overflow-x-auto bg-sidebar px-2.5 pt-1 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
          {tabs.map((tab) => (
            <TabButton
              key={tab.id}
              active={tab.active && !pending}
              label={tab.title || hostOf(tab.url)}
              loading={tab.loading}
              onPick={() => {
                setPendingTab(sessionId, false);
                if (!tab.active) void activateTab(sessionId, tab.id);
              }}
              onClose={() => void closeTab(sessionId, tab.id)}
            />
          ))}
          {pending && (
            <TabButton
              active
              label="New tab"
              loading={false}
              onPick={() => inputRef.current?.focus()}
              onClose={() => setPendingTab(sessionId, false)}
            />
          )}
          <Button
            variant="ghost"
            size="icon-sm"
            className="mb-0.5 shrink-0"
            aria-label="New tab"
            disabled={pending}
            onClick={newTab}
          >
            <Plus className="size-3.5" />
          </Button>
        </div>
      )}
      <div className="flex h-9 items-center gap-0.5 bg-card px-1.5">
        <Button
          variant="ghost"
          size="icon-sm"
          className={TOOL_BTN}
          aria-label="Back"
          disabled={!current}
          onClick={() => void navigate(sessionId, "back")}
        >
          <ArrowLeft className="size-3.5" />
        </Button>
        <Button
          variant="ghost"
          size="icon-sm"
          className={TOOL_BTN}
          aria-label="Forward"
          disabled={!current}
          onClick={() => void navigate(sessionId, "forward")}
        >
          <ArrowRight className="size-3.5" />
        </Button>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              variant="ghost"
              size="icon-sm"
              className={TOOL_BTN}
              aria-label={current?.loading ? "Stop" : "Reload"}
              disabled={!current}
              onClick={(e) =>
                void navigate(
                  sessionId,
                  current?.loading ? "stop" : e.shiftKey ? "hard_reload" : "reload",
                )
              }
            >
              {current?.loading ? <X className="size-3.5" /> : <RotateCw className="size-3.5" />}
            </Button>
          </TooltipTrigger>
          <TooltipContent side={TIP_SIDE}>
            {current?.loading ? "Stop" : "Reload · ⇧ for hard reload"}
          </TooltipContent>
        </Tooltip>
        <form
          className="relative min-w-0 flex-1"
          onSubmit={(e) => {
            e.preventDefault();
            open(draft ?? url, !current);
            setDraft(null);
            inputRef.current?.blur();
          }}
        >
          <input
            ref={inputRef}
            value={draft ?? url}
            placeholder="Search or enter a URL"
            spellCheck={false}
            autoCapitalize="off"
            autoCorrect="off"
            onChange={(e) => setDraft(e.target.value)}
            onFocus={(e) => e.target.select()}
            onBlur={() => setDraft(null)}
            className="h-7 w-full rounded-md bg-surface-raised px-2.5 font-mono dark:bg-background text-ui outline-none focus:ring-1 focus:ring-ring"
          />
        </form>
        {TOUCH && (
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label="Type into the page"
            aria-pressed={typing}
            disabled={!current}
            className={cn(TOOL_BTN, typing && "bg-foreground/10")}
            onClick={onToggleTyping}
          >
            <Keyboard className="size-3.5" />
          </Button>
        )}
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              variant="ghost"
              size="icon-sm"
              aria-label="Device size"
              aria-pressed={deviceBar}
              disabled={!current}
              className={cn(TOOL_BTN, deviceBar && "bg-foreground/10")}
              onClick={onToggleDeviceBar}
            >
              <Smartphone className="size-3.5" />
            </Button>
          </TooltipTrigger>
          <TooltipContent side={TIP_SIDE}>Device size</TooltipContent>
        </Tooltip>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              variant="ghost"
              size="icon-sm"
              className={TOOL_BTN}
              aria-label="Open in system browser"
              disabled={!current}
              onClick={() => void openUrl(url).catch(console.error)}
            >
              <ExternalLink className="size-3.5" />
            </Button>
          </TooltipTrigger>
          <TooltipContent side={TIP_SIDE}>Open in system browser</TooltipContent>
        </Tooltip>
        {swap && (
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                variant="ghost"
                size="icon-sm"
                className={TOOL_BTN}
                onClick={swap}
                aria-label={mode === "panel" ? "Open full view" : "Back to the panel"}
              >
                {mode === "panel" ? <Maximize2 className="size-3.5" /> : <Minimize2 className="size-3.5" />}
              </Button>
            </TooltipTrigger>
            <TooltipContent side={TIP_SIDE}>
              {mode === "panel" ? "Open full view" : "Back to the panel"}
            </TooltipContent>
          </Tooltip>
        )}
      </div>
      {openError && (
        <p className="bg-card px-3 pb-1.5 text-ui text-destructive">{openError}</p>
      )}
    </div>
  );
}

function TabButton({
  active,
  label,
  loading,
  onPick,
  onClose,
}: {
  active: boolean;
  label: string;
  loading: boolean;
  onPick: () => void;
  onClose: () => void;
}) {
  return (
    <div
      role="tab"
      aria-selected={active}
      tabIndex={0}
      onClick={onPick}
      onKeyDown={(e) => e.key === "Enter" && onPick()}
      className={cn(
        "group/tab flex h-7 w-40 min-w-0 shrink-0 cursor-default items-center gap-1.5 rounded-t-md px-2 text-ui",
        active
          ? "browser-tab-active bg-card text-foreground"
          : "text-muted-foreground hover:bg-card/50 hover:text-foreground",
      )}
    >
      {loading ? <Spinner className="size-3.5 shrink-0" /> : <Globe className="size-3.5 shrink-0 opacity-60" />}
      <span className="min-w-0 flex-1 truncate">{label}</span>
      <button
        type="button"
        aria-label="Close tab"
        className="rounded p-0.5 opacity-0 hover:bg-muted group-hover/tab:opacity-100"
        onClick={(e) => {
          e.stopPropagation();
          onClose();
        }}
      >
        <X className="size-3" />
      </button>
    </div>
  );
}

// Wide enough for four digits beside the placeholder, with the native
// stepper off — it sat on the placeholder and stepped by one pixel.
const SIZE_INPUT =
  "h-6 w-20 rounded-md bg-surface-raised dark:bg-background px-2 font-mono text-ui outline-none focus:ring-1 focus:ring-ring [appearance:textfield] [&::-webkit-inner-spin-button]:appearance-none [&::-webkit-outer-spin-button]:appearance-none";

/// Preset picker, free width and height, rotate. Responsive is the absence
/// of a viewport, so picking it clears rather than stores one.
function DeviceBar({
  sessionId,
  viewport,
  onClose,
}: {
  sessionId: string;
  viewport: Viewport | null;
  onClose: () => void;
}) {
  const apply = (next: Viewport | null) => setViewport(sessionId, next);
  const size = (axis: "width" | "height", raw: string) => {
    const n = Math.max(200, Math.min(4000, Math.round(Number(raw)) || 0));
    if (!n) return;
    apply({ preset: "custom", width: viewport?.width ?? n, height: viewport?.height ?? n, [axis]: n });
  };

  return (
    <div className="flex h-8 shrink-0 items-center gap-1.5 border-b border-border bg-card px-2 text-ui">
      <select
        value={viewport?.preset ?? "responsive"}
        aria-label="Device preset"
        onChange={(e) => {
          const preset = VIEWPORT_PRESETS.find((p) => p.id === e.target.value);
          apply(preset ? { preset: preset.id, width: preset.width, height: preset.height } : null);
        }}
        className="h-6 rounded-md bg-surface-raised dark:bg-background px-1.5 text-ui outline-none focus:ring-1 focus:ring-ring"
      >
        <option value="responsive">Responsive</option>
        {VIEWPORT_PRESETS.map((p) => (
          <option key={p.id} value={p.id}>
            {p.label}
          </option>
        ))}
        {viewport?.preset === "custom" && <option value="custom">Custom</option>}
      </select>
      <input
        type="number"
        aria-label="Viewport width"
        value={viewport?.width ?? ""}
        placeholder="width"
        onChange={(e) => size("width", e.target.value)}
        className={SIZE_INPUT}
      />
      <span className="text-muted-foreground">×</span>
      <input
        type="number"
        aria-label="Viewport height"
        value={viewport?.height ?? ""}
        placeholder="height"
        onChange={(e) => size("height", e.target.value)}
        className={SIZE_INPUT}
      />
      <Button
        variant="ghost"
        size="icon-sm"
        className={TOOL_BTN}
        aria-label="Rotate"
        disabled={!viewport}
        onClick={() =>
          viewport && apply({ preset: "custom", width: viewport.height, height: viewport.width })
        }
      >
        <RotateCcw className="size-3.5" />
      </Button>
      <Button
        variant="ghost"
        size="icon-sm"
        className={cn("ml-auto", TOOL_BTN)}
        aria-label="Close device toolbar"
        onClick={() => {
          apply(null);
          onClose();
        }}
      >
        <X className="size-3.5" />
      </Button>
    </div>
  );
}

/// What to open when nothing is: this checkout's dev servers, the session's
/// own marked. Polled while on screen, since a server starting is the
/// moment the list is looked at.
function Servers({ sessionId }: { sessionId: string }) {
  const [servers, setServers] = useState<LocalServer[]>([]);

  useEffect(() => {
    let live = true;
    const read = () =>
      void listLocalServers(sessionId)
        .then((list) => live && setServers(list))
        .catch(() => undefined);
    read();
    const timer = setInterval(read, 5000);
    return () => {
      live = false;
      clearInterval(timer);
    };
  }, [sessionId]);

  const open = (url: string) => void openInBrowser(sessionId, url, true).catch(() => undefined);

  return (
    <div className="flex h-full items-center justify-center p-8 text-ui">
      <div className="flex w-full max-w-sm flex-col gap-4">
        {servers.length > 0 && (
          <section className="flex flex-col gap-0.5">
            <h3 className="px-2 pb-1 text-muted-foreground">Running locally</h3>
            {servers.map((s) => (
              <button
                key={s.port}
                type="button"
                onClick={() => open(`http://localhost:${s.port}`)}
                className="flex items-center gap-2 rounded-md px-2 py-1.5 text-left hover:bg-muted"
              >
                <span className="font-mono">localhost:{s.port}</span>
                <span className="truncate text-muted-foreground">{s.process}</span>
                {s.mine && <span className="ml-auto text-muted-foreground">this session</span>}
              </button>
            ))}
          </section>
        )}
        <p className="px-2 text-muted-foreground">
          Search or enter a URL above, or open a link from the chat.
        </p>
      </div>
    </div>
  );
}

function hostOf(url: string) {
  try {
    return new URL(url).host || url;
  } catch {
    return url || "New tab";
  }
}
