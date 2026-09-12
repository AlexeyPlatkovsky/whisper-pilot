import React from "react";
import ReactDOM from "react-dom/client";
import { App } from "./App";
import { RecorderCaption } from "./RecorderCaption";
import { RecorderBubble } from "./RecorderBubble";
import "./tokens.css";
import "./styles.css";

const isRecorderCaption =
  new URLSearchParams(window.location.search).get("window") ===
  "recorder-caption";
const isRecorderBubble =
  new URLSearchParams(window.location.search).get("window") ===
  "recorder-bubble";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    {isRecorderBubble ? (
      <RecorderBubble />
    ) : isRecorderCaption ? (
      <RecorderCaption />
    ) : (
      <App />
    )}
  </React.StrictMode>,
);
