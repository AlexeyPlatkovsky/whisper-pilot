import React from "react";
import ReactDOM from "react-dom/client";
import { App } from "./App";
import { RecorderCaption } from "./RecorderCaption";
import "./tokens.css";
import "./styles.css";

const isRecorderCaption =
  new URLSearchParams(window.location.search).get("window") ===
  "recorder-caption";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    {isRecorderCaption ? <RecorderCaption /> : <App />}
  </React.StrictMode>,
);
