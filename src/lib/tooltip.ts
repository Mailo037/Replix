// One tooltip for the whole app. It sits in the top layer, so no panel or popover clips it.
let box: HTMLDivElement | null = null;
let owner: HTMLElement | null = null;
let timer = 0;
/** Moving from one tooltip to the next shows the next one at once. */
let lastHidden = 0;

function element(): HTMLDivElement {
  if (!box) {
    box = document.createElement("div");
    box.className = "tooltip";
    box.setAttribute("role", "tooltip");
    box.popover = "manual";
    document.body.append(box);
  }
  return box;
}

function place(node: HTMLElement, el: HTMLDivElement) {
  const r = node.getBoundingClientRect();
  const t = el.getBoundingClientRect();
  const above = r.top - t.height - 8;
  const top = above >= 4 ? above : r.bottom + 8;
  const left = Math.min(Math.max(4, r.left + r.width / 2 - t.width / 2), innerWidth - t.width - 4);
  el.style.left = `${Math.round(left)}px`;
  el.style.top = `${Math.round(top)}px`;
}

function show(node: HTMLElement, text: string, delay: number) {
  clearTimeout(timer);
  timer = window.setTimeout(() => {
    if (!node.isConnected) return;
    const el = element();
    owner = node;
    el.textContent = text;
    if (!el.matches(":popover-open")) el.showPopover();
    place(node, el);
  }, delay);
}

function hide() {
  clearTimeout(timer);
  if (box?.matches(":popover-open")) {
    box.hidePopover();
    lastHidden = Date.now();
  }
  owner = null;
}

addEventListener("scroll", hide, true);
addEventListener("blur", hide);

/** `use:tip={"Text"}` shows the text above the element on hover or keyboard focus. */
export function tip(node: HTMLElement, text: string | null | undefined) {
  let current = text;
  const delay = () => (Date.now() - lastHidden < 400 ? 0 : 500);
  const enter = () => current && show(node, current, delay());
  const focus = () => current && node.matches(":focus-visible") && show(node, current, 0);
  const key = (e: KeyboardEvent) => e.key === "Escape" && hide();

  node.addEventListener("pointerenter", enter);
  node.addEventListener("pointerleave", hide);
  node.addEventListener("pointerdown", hide);
  node.addEventListener("focus", focus);
  node.addEventListener("blur", hide);
  node.addEventListener("keydown", key);

  return {
    update(next: string | null | undefined) {
      current = next;
      if (owner !== node) return;
      if (!next) hide();
      else if (box) {
        box.textContent = next;
        place(node, box);
      }
    },
    destroy() {
      node.removeEventListener("pointerenter", enter);
      node.removeEventListener("pointerleave", hide);
      node.removeEventListener("pointerdown", hide);
      node.removeEventListener("focus", focus);
      node.removeEventListener("blur", hide);
      node.removeEventListener("keydown", key);
      if (owner === node) hide();
    },
  };
}
