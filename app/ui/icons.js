// The panel's one icon family: line icons on a 24-unit grid, drawn with the
// stroke set in CSS (.i), so every icon has the same weight. The sprite is
// added to the page once; icon() refers to it.

const PATHS = {
  clock: '<circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/>',
  sun: '<circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/>',
  branch: '<circle cx="6" cy="5" r="2"/><circle cx="6" cy="19" r="2"/><circle cx="18" cy="7" r="2"/><path d="M6 7v10M18 9c0 5-7 4-11 8"/>',
  chart: '<path d="M5 20V11M12 20V5M19 20v-6"/>',
  more: '<circle cx="5" cy="12" r="1.3"/><circle cx="12" cy="12" r="1.3"/><circle cx="19" cy="12" r="1.3"/>',
  back: '<path d="M15 5l-7 7 7 7"/>',
  arrow: '<path d="M9 5l7 7-7 7"/>',
  copy: '<rect x="8" y="8" width="12" height="12" rx="2"/><path d="M16 8V5a1 1 0 0 0-1-1H5a1 1 0 0 0-1 1v10a1 1 0 0 0 1 1h3"/>',
  spark: '<path d="M12 3l1.8 5.2L19 10l-5.2 1.8L12 17l-1.8-5.2L5 10l5.2-1.8z"/>',
  down: '<path d="M12 4v12M6 11l6 6 6-6M5 20h14"/>',
  term: '<rect x="3" y="4.5" width="18" height="15" rx="2"/><path d="M7 9.5l3 2.5-3 2.5M12.5 15h4"/>',
  play: '<path d="M7 5l12 7-12 7z"/>',
  close: '<path d="M6 6l12 12M18 6L6 18"/>',
  trash: '<path d="M4 7h16M10 11v6M14 11v6M6 7l1 12a1 1 0 0 0 1 1h8a1 1 0 0 0 1-1l1-12M9 7V4h6v3"/>',
};

export const ICONS = Object.keys(PATHS);

export function sprite() {
  const symbols = ICONS.map((n) => `<symbol id="i-${n}" viewBox="0 0 24 24">${PATHS[n]}</symbol>`).join('');
  return `<svg xmlns="http://www.w3.org/2000/svg" width="0" height="0" style="position:absolute" aria-hidden="true"><defs>${symbols}</defs></svg>`;
}

// size: '' for 18 px (tab bar, header), 's' for 14 px inline.
export function icon(name, size = '') {
  if (!PATHS[name]) throw new Error(`unknown icon: ${name}`);
  return `<svg class="${size ? `i ${size}` : 'i'}" aria-hidden="true"><use href="#i-${name}"></use></svg>`;
}
