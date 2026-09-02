import "./styles.css";
import "./trace-timeline-view.css";

import { installTraceDevMock } from "./trace-dev-mock";
import { TraceViewer } from "./trace-timeline-view";

installTraceDevMock();
document.body.innerHTML = '<main><section id="trace-preview-root"></section></main>';
const root = document.querySelector<HTMLElement>("#trace-preview-root");
if (!root) throw new Error("Trace preview root is missing");
const viewer = new TraceViewer(root);
void viewer.activate();
