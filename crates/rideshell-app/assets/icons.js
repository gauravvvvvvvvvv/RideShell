// RideShell UI icons: 24x24 line icons, stroke = currentColor.
(function () {
  const s = (body, extra = '') =>
    `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" ${extra}>${body}</svg>`;
  window.ICONS = {
    logo: `<img src="/icon.png" alt="" draggable="false">`,
    plus: s('<path d="M12 5v14M5 12h14"/>'),
    play: s('<path d="M8 5.5v13l10-6.5z" fill="currentColor"/>'),
    pause: s('<path d="M8 5v14M16 5v14" stroke-width="3"/>'),
    next: s('<path d="M6 5.5v13l9-6.5z" fill="currentColor"/><path d="M18 5v14" stroke-width="2.4"/>'),
    prev: s('<path d="M18 5.5v13l-9-6.5z" fill="currentColor"/><path d="M6 5v14" stroke-width="2.4"/>'),
    close: s('<path d="M6 6l12 12M18 6L6 18"/>'),
    min: s('<path d="M5 12h14"/>'),
    max: s('<rect x="5" y="5" width="14" height="14" rx="2.5"/>'),
    search: s('<circle cx="11" cy="11" r="6.5"/><path d="M20 20l-4.2-4.2"/>'),
    shield: s('<path d="M12 3l7.5 3v5.5c0 4.6-3.2 8.3-7.5 9.5-4.3-1.2-7.5-4.9-7.5-9.5V6z"/><path d="M9 12l2.2 2.2L15.5 10"/>'),
    gear: s('<circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z"/>'),
    sidebar: s('<rect x="3.5" y="4.5" width="17" height="15" rx="3"/><path d="M9.5 4.5v15"/>'),
    folder: s('<path d="M3.5 7.5a2 2 0 0 1 2-2h4l2 2.2h7a2 2 0 0 1 2 2v7.8a2 2 0 0 1-2 2h-13a2 2 0 0 1-2-2z"/>'),
    terminal: s('<rect x="3" y="4.5" width="18" height="15" rx="3"/><path d="M7.5 10l2.5 2.2-2.5 2.3M12.5 15h4"/>'),
    sparkle: s('<path d="M12 3.5l1.6 4.6a3 3 0 0 0 1.9 1.9l4.5 1.5-4.5 1.6a3 3 0 0 0-1.9 1.9L12 19.5l-1.6-4.5a3 3 0 0 0-1.9-1.9L4 11.5l4.5-1.5a3 3 0 0 0 1.9-1.9z"/>'),
    key: s('<circle cx="8" cy="15" r="4"/><path d="M10.8 12.2L20 3M16 7l3 3M14 9l2 2"/>'),
    login: s('<path d="M14 4h3.5a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2H14"/><path d="M9.5 16.5L14 12 9.5 7.5M14 12H3.5"/>'),
    pulse: s('<path d="M3 12h4l2.5-6 5 12 2.5-6h4"/>'),
    star: s('<path d="M12 3.8l2.5 5.1 5.6.8-4 4 .9 5.6-5-2.7-5 2.7.9-5.6-4-4 5.6-.8z"/>'),
    external: s('<path d="M14 4h6v6M20 4l-9 9"/><path d="M18 14v4a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h4"/>'),
    check: s('<path d="M5 12.5l4.5 4.5L19 7.5"/>'),
    user: s('<circle cx="12" cy="8.5" r="3.8"/><path d="M4.5 20a7.5 7.5 0 0 1 15 0"/>'),
    bolt: s('<path d="M13 2.5L4.5 13.5H12l-1 8 8.5-11H12z"/>'),
    app: s('<rect x="4" y="4" width="16" height="16" rx="4"/><path d="M4 9h16"/>'),
    doc: s('<path d="M7 3.5h6.5L18 8v12a1 1 0 0 1-1 1H7a1 1 0 0 1-1-1v-15.5a1 1 0 0 1 1-1z"/><path d="M13.5 3.5V8H18"/>'),
    chevron: s('<path d="M9 6l6 6-6 6"/>'),
  };
})();
