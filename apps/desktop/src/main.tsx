import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import ControlRoom from "./components/control-room/ControlRoom";
import "./styles.css";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <ControlRoom />
  </StrictMode>,
);

