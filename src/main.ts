import { mount } from "svelte";
import "./app.css";

const isToast = location.hash === "#toast";
if (isToast) document.body.classList.add("toast-body");

// No browser menu on right-click; the app shows its own where one makes sense.
document.addEventListener("contextmenu", (e) => e.preventDefault());

// Nor browser shortcuts: find bar, print, reload, zoom, back and forward, view source and
// so on. Ctrl with these keys still edits text; the app's own shortcuts still see every
// key, as only the browser's default is stopped.
const EDITING = new Set(["c", "v", "x", "a", "z", "y", "backspace", "delete", "arrowleft", "arrowright", "arrowup", "arrowdown", "home", "end"]);
window.addEventListener(
  "keydown",
  (e) => {
    const key = e.key.toLowerCase();
    const browserKey =
      ((e.ctrlKey || e.metaKey) && !EDITING.has(key) && !["control", "shift", "alt", "meta"].includes(key)) ||
      (e.altKey && (key === "arrowleft" || key === "arrowright" || key === "home")) ||
      (/^f\d{1,2}$/.test(key) && !(import.meta.env.DEV && key === "f12")) ||
      key === "browserback" ||
      key === "browserforward" ||
      key === "browserrefresh";
    if (browserKey) e.preventDefault();
  },
  { capture: true },
);
// Ctrl+wheel would zoom the whole page.
window.addEventListener("wheel", (e) => e.ctrlKey && e.preventDefault(), { passive: false, capture: true });
// The mouse's back and forward buttons.
window.addEventListener("mouseup", (e) => (e.button === 3 || e.button === 4) && e.preventDefault(), { capture: true });

// Each window loads only its own page. The toast window stays loaded for as long as Replix
// runs, and has no use for the library and the editor.
const page = isToast ? await import("./components/Toast.svelte") : await import("./App.svelte");

export default mount(page.default, { target: document.getElementById("app")! });
