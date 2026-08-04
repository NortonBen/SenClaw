import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { installExternalLinkHook } from "./openExternal";

// Every off-origin <a> click opens in the system browser (desktop webview
// would otherwise navigate the embedded frame away from the app).
installExternalLinkHook();

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
