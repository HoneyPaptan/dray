import React from "react";
import ReactDOM from "react-dom/client";
import "streamdown/styles.css";
import App from "./App";
import { RemoteGate } from "@/components/RemoteGate";
import { trackActiveDay } from "@/lib/analytics";
import { installBackHandler } from "@/lib/backStack";
import { onFocusChange } from "@/lib/focus";
import { startRemoteExecutor } from "@/lib/remoteExecutor";
import { startSurveys } from "@/lib/surveys";
import { watchViewport } from "@/lib/viewport";

// Coming back to check on a session an agent is running sends no prompt and
// starts nothing, so it is the one kind of use the backend's own call sites
// cannot see. Subscribed here rather than from an effect: it lives for the
// process, and StrictMode double-invokes a mount.
onFocusChange((focused) => {
  if (focused) trackActiveDay();
});

// Here for the same reason, and not in an effect for the same reason: the SDK
// lives for the process, and a mount that runs twice would initialise it twice.
// It refuses itself where the install has opted out, so this is unconditional.
void startSurveys();

// The desktop window is what runs a phone's commands, so this has to be up for
// the whole process rather than for as long as a component is mounted.
startRemoteExecutor();

// Android's Back is answered by the frontend, which is the only side that
// knows what is open. Installed for the process: the shell asks `window` and
// the answer must be there before the first press, not after a mount.
installBackHandler();

// The soft keyboard changes how much of the window the app may draw in, and
// nothing in CSS can measure it. Started here for the process, like the two
// subscriptions above.
watchViewport();

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <RemoteGate>
      <App />
    </RemoteGate>
  </React.StrictMode>,
);
