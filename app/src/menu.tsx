// Context menus (right click, or Shift+F10 / the Menu key on a focused item).

import { signal } from "@preact/signals";
import { useEffect, useRef } from "preact/hooks";

export type MenuItem = { label: string; run: () => void; disabled?: boolean } | "separator";

const open = signal<{ x: number; y: number; items: MenuItem[]; label: string } | null>(null);

/** Shows a context menu at the pointer (or at an element, for the keyboard). */
export function showContextMenu(e: MouseEvent | KeyboardEvent, label: string, items: MenuItem[]) {
  e.preventDefault();
  e.stopPropagation();
  let x: number, y: number;
  if ("clientX" in e && (e.clientX || e.clientY)) { x = e.clientX; y = e.clientY; }
  else {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    x = r.left + 8; y = r.bottom;
  }
  open.value = { x, y, items, label };
}

/** Whether a key event asks for the context menu. */
export function isMenuKey(e: KeyboardEvent): boolean {
  return e.key === "ContextMenu" || (e.key === "F10" && e.shiftKey);
}

export function ContextMenu() {
  const m = open.value;
  const ref = useRef<HTMLUListElement>(null);
  useEffect(() => {
    if (!m) return;
    const first = ref.current?.querySelector<HTMLButtonElement>("button:not([disabled])");
    first?.focus();
    const close = (e: Event) => { if (!ref.current?.contains(e.target as Node)) open.value = null; };
    window.addEventListener("pointerdown", close, true);
    window.addEventListener("blur", () => { open.value = null; }, { once: true });
    return () => window.removeEventListener("pointerdown", close, true);
  }, [m]);
  if (!m) return null;
  // Keep the menu on screen.
  const x = Math.min(m.x, window.innerWidth - 240);
  const y = Math.min(m.y, window.innerHeight - 30 * m.items.length - 10);
  const onKey = (e: KeyboardEvent) => {
    const buttons = [...(ref.current?.querySelectorAll<HTMLButtonElement>("button:not([disabled])") ?? [])];
    const i = buttons.indexOf(document.activeElement as HTMLButtonElement);
    if (e.key === "Escape") { e.preventDefault(); open.value = null; }
    if (e.key === "ArrowDown") { e.preventDefault(); buttons[(i + 1) % buttons.length]?.focus(); }
    if (e.key === "ArrowUp") { e.preventDefault(); buttons[(i - 1 + buttons.length) % buttons.length]?.focus(); }
  };
  return (
    <ul class="context-menu" role="menu" aria-label={m.label} ref={ref} style={{ left: `${x}px`, top: `${Math.max(4, y)}px` }} onKeyDown={onKey}>
      {m.items.map((it) => it === "separator"
        ? <li role="separator" class="sep" />
        : <li role="none"><button role="menuitem" disabled={it.disabled} onClick={() => { open.value = null; it.run(); }}>{it.label}</button></li>)}
    </ul>
  );
}
