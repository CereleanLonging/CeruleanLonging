import React from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { ReviewLogs } from "./ReviewLogs";
import { CliSessionWindow } from "./CliSessionWindow";
import "./style.css";
import { initializeAccessStorage } from "./accessStorage";
if(!["#review-logs", "#cli-session"].includes(window.location.hash))await initializeAccessStorage();
createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    {window.location.hash === "#review-logs" ? <ReviewLogs /> : window.location.hash === "#cli-session" ? <CliSessionWindow /> : <App />}
  </React.StrictMode>,
);
