// Keeps a popover (absolutely placed above its button: `bottom: calc(100% + 8px); right: 0`)
// entirely visible: it opens below the button when there's more room there, its height is limited
// to the room it has, and it's shifted sideways to stay inside the visible area (the window, the
// scrolling page or the fullscreen player, whichever clips it).
const MARGIN = 8;

function clipRect(el: HTMLElement): { top: number; bottom: number; left: number; right: number } {
  let r = { top: 0, bottom: window.innerHeight, left: 0, right: window.innerWidth };
  for (let e = el.parentElement; e; e = e.parentElement) {
    const s = getComputedStyle(e);
    if (s.overflowX === "visible" && s.overflowY === "visible") continue;
    const b = e.getBoundingClientRect();
    r = {
      top: Math.max(r.top, b.top + e.clientTop),
      bottom: Math.min(r.bottom, b.top + e.clientTop + e.clientHeight),
      left: Math.max(r.left, b.left + e.clientLeft),
      right: Math.min(r.right, b.left + e.clientLeft + e.clientWidth),
    };
  }
  return r;
}

export function popfit(el: HTMLElement) {
  let raf = 0;
  const place = () => {
    raf = 0;
    const host = el.offsetParent as HTMLElement | null;
    if (!host) return;
    el.style.top = "";
    el.style.bottom = "";
    el.style.right = "";
    el.style.maxHeight = "none";
    const natural = el.scrollHeight;
    const c = clipRect(el);
    const a = host.getBoundingClientRect();
    const above = a.top - c.top - MARGIN * 2;
    const below = c.bottom - a.bottom - MARGIN * 2;
    const cap = 480;
    if (above >= Math.min(natural, cap) || above >= below) {
      el.style.maxHeight = `${Math.max(80, Math.min(cap, above))}px`;
    } else {
      el.style.bottom = "auto";
      el.style.top = `calc(100% + ${MARGIN}px)`;
      el.style.maxHeight = `${Math.max(80, Math.min(cap, below))}px`;
    }
    // Sideways: inside the visible area.
    const r = el.getBoundingClientRect();
    let shift = 0;
    if (r.right > c.right - MARGIN) shift = c.right - MARGIN - r.right;
    if (r.left + shift < c.left + MARGIN) shift = c.left + MARGIN - r.left;
    if (shift) el.style.right = `${-shift}px`;
  };
  const later = () => {
    if (!raf) raf = requestAnimationFrame(place);
  };
  place();
  const ro = new ResizeObserver(later);
  ro.observe(el);
  window.addEventListener("resize", later);
  document.addEventListener("scroll", later, true);
  return {
    destroy() {
      cancelAnimationFrame(raf);
      ro.disconnect();
      window.removeEventListener("resize", later);
      document.removeEventListener("scroll", later, true);
    },
  };
}
