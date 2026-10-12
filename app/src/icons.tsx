// Simple line icons drawn for Logos. Decorative: every control that uses one also has a text
// label or an aria-label.

const paths: Record<string, string> = {
  model: "M3 3h10v10H3z M3 7h10 M7 7v6",
  files: "M2 4h5l1 1.5h6V13H2z",
  search: "M7 3a4 4 0 1 1 0 8 4 4 0 0 1 0-8z M10 10l3.5 3.5",
  experiments: "M6 2v4L3 13h10L10 6V2 M5 2h6 M4.5 10h7",
  results: "M3 13V3 M3 13h10 M5.5 10l2.5-3 2 2 3-4",
  llm: "M3 4h10v7H8l-3 2.5V11H3z M5.5 7.5h5",
  git: "M5 3v10 M5 9a3 3 0 0 0 3 -3h1 M11 3a2 2 0 1 1 0 4 2 2 0 0 1 0-4z M5 13a1.5 1.5 0 1 0 0-.1",
  play: "M5 3l8 5-8 5z",
  pause: "M5 3v10 M11 3v10",
  step: "M4 3l6 5-6 5z M12 3v10",
  stop: "M4 4h8v8H4z",
  forever: "M12.5 6.5A5 5 0 1 0 13 9 M13 3.5v3h-3",
  close: "M4 4l8 8 M12 4l-8 8",
  popout: "M9 3h4v4 M13 3L8 8 M11 9v4H3V5h4",
  inspector: "M3 3h10v10H3z M5 6h6 M5 8.5h6 M5 11h4",
  gear: "M8 5.5a2.5 2.5 0 1 1 0 5 2.5 2.5 0 0 1 0-5z M8 1.5v2 M8 12.5v2 M1.5 8h2 M12.5 8h2 M3.4 3.4l1.4 1.4 M11.2 11.2l1.4 1.4 M3.4 12.6l1.4-1.4 M11.2 4.8l1.4-1.4",
  edit: "M3 13l1-3 7-7 2 2-7 7z",
  add: "M8 3v10 M3 8h10",
  check: "M3 8.5l3 3 7-7",
  palette: "M3 4h10 M3 8h10 M3 12h6",
  panelBottom: "M2.5 2.5h11v11h-11z M2.5 10h11",
  panelRight: "M2.5 2.5h11v11h-11z M10 2.5v11",
};

export function Icon({ name, title }: { name: keyof typeof paths | string; title?: string }) {
  return (
    <svg class="icon" viewBox="0 0 16 16" aria-hidden={title ? undefined : "true"} role={title ? "img" : undefined}>
      {title && <title>{title}</title>}
      <path d={paths[name] ?? ""} />
    </svg>
  );
}
