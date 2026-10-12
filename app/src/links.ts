// Links open in the system's browser, never inside the Logos window.

import { openUrl } from "@tauri-apps/plugin-opener";

export function open(url: string) {
  if (!/^https?:\/\//i.test(url)) return;
  openUrl(url).catch(() => {});
}

/** Sends clicks on links in rendered text (Info tab, notes) to the browser. */
export function interceptLinks() {
  document.addEventListener("click", (e) => {
    const a = (e.target as HTMLElement | null)?.closest?.("a[href]") as HTMLAnchorElement | null;
    if (!a) return;
    e.preventDefault();
    open(a.getAttribute("href") ?? "");
  });
}
