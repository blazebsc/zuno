import React from "react";
import ReactDOM from "react-dom/client";
import { Analytics } from "@vercel/analytics/react";
import "@fontsource-variable/inter/opsz.css";
import "@fontsource-variable/geist-mono";
import { App } from "./App";
import "./styles.css";

// One theme on the site; the token layer still keys off data-theme.
document.documentElement.dataset.theme = "dark";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
    <Analytics />
  </React.StrictMode>,
);
