// Author fingerprint: a 5 x 5 mirrored grid drawn from the author's public
// key. Two authors can share a display name but not a key, so the glyph helps
// people notice when a familiar name has a different key.

import { h } from './dom.js';

export function fingerprint(authorHex, size = 20) {
  const cells = [];
  const bits = [];
  for (let i = 0; i < 4 && i * 2 < (authorHex || '').length; i++) {
    const byte = parseInt(authorHex.slice(i * 2, i * 2 + 2), 16);
    for (let b = 7; b >= 0; b--) bits.push((byte >> b) & 1);
  }
  // 15 cells (3 columns x 5 rows), mirrored to 5 columns.
  for (let row = 0; row < 5; row++) {
    for (let col = 0; col < 3; col++) {
      if (!bits[row * 3 + col]) continue;
      cells.push([col, row]);
      if (col < 2) cells.push([4 - col, row]);
    }
  }
  const svg = h('svg:svg', {
    class: 'fingerprint',
    width: size,
    height: size,
    viewBox: '0 0 5 5',
    'aria-hidden': 'true',
    focusable: 'false',
  });
  svg.append(h('svg:rect', { x: 0, y: 0, width: 5, height: 5, class: 'fp-bg' }));
  for (const [x, y] of cells) {
    svg.append(h('svg:rect', { x, y, width: 1, height: 1, class: 'fp-cell' }));
  }
  return svg;
}
