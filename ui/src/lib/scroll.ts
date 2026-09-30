// Remembers each page's scroll position, so going back to a page lands where you were.
const saved = new Map<string, number>();

export function keepScroll(node: HTMLElement, key: string) {
  const top = saved.get(key);
  if (top) requestAnimationFrame(() => (node.scrollTop = top));
  const onScroll = () => saved.set(key, node.scrollTop);
  node.addEventListener("scroll", onScroll, { passive: true });
  return {
    destroy() {
      node.removeEventListener("scroll", onScroll);
    },
  };
}
