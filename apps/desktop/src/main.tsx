import React from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { ReviewLogs } from "./ReviewLogs";
import "./style.css";
import { initializeAccessStorage } from "./accessStorage";
if(window.location.hash!=="#review-logs")await initializeAccessStorage();
createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    {window.location.hash === "#review-logs" ? <ReviewLogs /> : <App />}
  </React.StrictMode>,
);
