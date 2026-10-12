// The command palette (design principle 7): every action, searchable by name.

import { useEffect, useRef, useState } from "preact/hooks";
import { COMMANDS, runCommand } from "./commands";
import { paletteOpen } from "./state";

export function Palette() {
  const [q, setQ] = useState("");
  const [sel, setSel] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => input.current?.focus(), []);
  const words = q.toLowerCase().split(/\s+/).filter(Boolean);
  const list = COMMANDS.filter((c) => words.every((w) => `${c.menu} ${c.title}`.toLowerCase().includes(w)));
  const close = () => { paletteOpen.value = false; };
  const choose = (i: number) => {
    const c = list[i];
    if (!c) return;
    close();
    runCommand(c);
  };
  return (
    <div class="overlay" onClick={close}>
      <div class="palette" role="dialog" aria-label="Commands" onClick={(e) => e.stopPropagation()}>
        <input ref={input} type="text" value={q} placeholder="Type a command" aria-label="Search commands"
          aria-controls="palette-list" aria-activedescendant={list[sel] ? `cmd-${list[sel].id}` : undefined}
          onInput={(e) => { setQ((e.target as HTMLInputElement).value); setSel(0); }}
          onKeyDown={(e) => {
            if (e.key === "Escape") close();
            if (e.key === "ArrowDown") { setSel(Math.min(sel + 1, list.length - 1)); e.preventDefault(); }
            if (e.key === "ArrowUp") { setSel(Math.max(sel - 1, 0)); e.preventDefault(); }
            if (e.key === "Enter") choose(sel);
          }} />
        <ul id="palette-list" role="listbox">
          {list.map((c, i) => {
            const disabled = c.enabled ? !c.enabled() : false;
            return (
              <li id={`cmd-${c.id}`} role="option" aria-selected={i === sel} aria-disabled={disabled}
                style={disabled ? "opacity: 0.5" : undefined} onClick={() => choose(i)} onMouseEnter={() => setSel(i)}>
                <span><span class="muted">{c.menu}: </span>{c.title}</span>
                {c.key && <span class="kbd">{c.key}</span>}
              </li>
            );
          })}
        </ul>
      </div>
    </div>
  );
}
