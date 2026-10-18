// Price history chart: inline SVG, no library.
//
// One chart per currency (one y-axis). Each listing is a series drawn as a
// step line, because a seller's price holds until the seller changes it.
// Seller price records are circles; prices observed by others are diamonds,
// so the two sources differ by shape as well as by position. Series colors
// are fixed slots assigned by listing ID, so a listing keeps its color when
// others are added. Pointer and arrow keys move a crosshair that reports
// every series at that date. A table lists every point.

import { h } from './dom.js';
import * as fmt from './format.js';

const SLOTS = 4; // validated categorical slots (see styles.css --series-*)
const M = { top: 16, right: 16, bottom: 28, left: 64 };

function niceStep(range, target) {
  const raw = range / Math.max(1, target);
  const mag = 10 ** Math.floor(Math.log10(raw || 1));
  for (const m of [1, 2, 2.5, 5, 10]) if (raw <= m * mag) return m * mag;
  return 10 * mag;
}

function yTicks(min, max) {
  if (min === max) {
    min = Math.max(0, min - Math.max(1, Math.round(min * 0.1)));
    max = max + Math.max(1, Math.round(max * 0.1));
  }
  const step = niceStep(max - min, 4);
  const lo = Math.floor(min / step) * step;
  const hi = Math.ceil(max / step) * step;
  const ticks = [];
  for (let v = lo; v <= hi + step / 2; v += step) ticks.push(Math.round(v));
  return { lo, hi, ticks };
}

function xTicks(t0, t1, width) {
  const ticks = [];
  const d = new Date(t0);
  d.setUTCDate(1);
  d.setUTCHours(0, 0, 0, 0);
  const spanDays = (t1 - t0) / 86400000;
  const everyMonths = spanDays > 720 ? 6 : spanDays > 300 ? 2 : 1;
  const maxTicks = Math.max(2, Math.floor(width / 70));
  let step = everyMonths;
  for (;;) {
    ticks.length = 0;
    const c = new Date(d);
    while (c.getTime() <= t1) {
      if (c.getTime() >= t0) ticks.push(c.getTime());
      c.setUTCMonth(c.getUTCMonth() + step);
    }
    if (ticks.length <= maxTicks) break;
    step *= 2;
  }
  return ticks;
}

// Price of a series at time t (step function over seller points), or null.
function valueAt(series, t) {
  let v = null;
  for (const p of series.steps) {
    if (p.ts <= t) v = p.amount;
    else break;
  }
  return v;
}

export function seriesFor(listings, currency) {
  const out = [];
  const sorted = [...listings].sort((a, b) => (a.id < b.id ? -1 : 1));
  sorted.forEach((l, i) => {
    const points = l.price_history.filter((p) => p.currency === currency);
    if (!points.length) return;
    out.push({
      listing: l,
      slot: (i % SLOTS) + 1,
      label: l.seller_name || fmt.shortHex(l.seller),
      steps: points.filter((p) => p.source === 'seller'),
      observations: points.filter((p) => p.source === 'observation'),
      points,
    });
  });
  return out.slice(0, SLOTS);
}

export function priceChart({ listings, currency, now, title }) {
  const series = seriesFor(listings, currency);
  const root = h('figure', { class: 'chart', dataset: { currency } });
  const heading = h('figcaption', { class: 'chart-title' }, title || `Price history in ${currency}`);
  root.append(heading);
  if (!series.length) {
    root.append(h('p', { class: 'muted', text: 'No price history in this currency.' }));
    return root;
  }
  const omitted = listings.filter((l) => l.price_history.some((p) => p.currency === currency)).length - series.length;

  const allPoints = series.flatMap((s) => s.points);
  const tMin = Math.min(...allPoints.map((p) => p.ts));
  const tMax = Math.max(now ?? 0, ...allPoints.map((p) => p.ts));
  const yMin = Math.min(...allPoints.map((p) => p.amount));
  const yMax = Math.max(...allPoints.map((p) => p.amount));
  const yt = yTicks(yMin, yMax);

  // Legend: always present for 2+ series; one series needs none.
  if (series.length > 1) {
    root.append(
      h('ul', { class: 'chart-legend' },
        series.map((s) =>
          h('li', {},
            h('span', { class: `key-line series-${s.slot}`, 'aria-hidden': 'true' }),
            h('bdi', { text: s.label }),
            s.listing.domain_verified ? null : h('span', { class: 'muted', text: ' (domain not verified)' }),
          ),
        ),
      ),
    );
  }
  root.append(
    h('p', { class: 'chart-key' },
      h('span', { class: 'key-circle', 'aria-hidden': 'true' }), ' seller price record ',
      h('span', { class: 'key-diamond', 'aria-hidden': 'true' }), ' price observed by others',
    ),
  );

  const plot = h('div', { class: 'chart-plot' });
  root.append(plot);
  const tooltip = h('div', { class: 'chart-tooltip', role: 'status', 'aria-live': 'polite', hidden: true });

  const positions = [...new Set(allPoints.map((p) => p.ts))].sort((a, b) => a - b);

  let width = 0;
  let svg = null;
  let x = null;
  let y = null;
  let focusIndex = null;

  const draw = () => {
    width = Math.max(280, Math.floor(plot.clientWidth || 640));
    const height = 220;
    const iw = width - M.left - M.right;
    const ih = height - M.top - M.bottom;
    const span = Math.max(1, tMax - tMin);
    x = (t) => M.left + ((t - tMin) / span) * iw;
    y = (v) => M.top + ih - ((v - yt.lo) / Math.max(1, yt.hi - yt.lo)) * ih;

    svg = h('svg:svg', {
      class: 'chart-svg',
      width,
      height,
      viewBox: `0 0 ${width} ${height}`,
      role: 'img',
      'aria-label': `Price history in ${currency}, ${series.length} ${series.length === 1 ? 'listing' : 'listings'}. Use the table below for all values.`,
    });
    const grid = h('svg:g', { class: 'grid' });
    for (const v of yt.ticks) {
      grid.append(h('svg:line', { x1: M.left, x2: width - M.right, y1: y(v), y2: y(v) }));
      grid.append(h('svg:text', { x: M.left - 8, y: y(v), class: 'tick y', 'text-anchor': 'end', 'dominant-baseline': 'middle', text: fmt.money(v, currency) }));
    }
    for (const t of xTicks(tMin, tMax, iw)) {
      grid.append(h('svg:text', { x: x(t), y: height - 8, class: 'tick x', 'text-anchor': 'middle', text: fmt.monthYear(t) }));
    }
    grid.append(h('svg:line', { class: 'baseline', x1: M.left, x2: width - M.right, y1: M.top + ih, y2: M.top + ih }));
    svg.append(grid);

    for (const s of series) {
      const g = h('svg:g', { class: `series series-${s.slot}`, dataset: { listing: s.listing.id } });
      if (s.steps.length) {
        let d = '';
        s.steps.forEach((p, i) => {
          const px = x(p.ts);
          const py = y(p.amount);
          if (i === 0) d += `M${px},${py}`;
          else d += `H${px}V${py}`;
        });
        const endT = s.listing.status === 'visible' ? tMax : s.steps[s.steps.length - 1].ts;
        d += `H${x(endT)}`;
        g.append(h('svg:path', { class: 'line', d }));
      }
      for (const p of s.steps) g.append(h('svg:circle', { class: 'dot', cx: x(p.ts), cy: y(p.amount), r: 4 }));
      for (const p of s.observations) {
        const cx = x(p.ts);
        const cy = y(p.amount);
        g.append(h('svg:path', { class: 'dot diamond', d: `M${cx},${cy - 5}L${cx + 5},${cy}L${cx},${cy + 5}L${cx - 5},${cy}Z` }));
      }
      svg.append(g);
    }

    // Direct label at the end of a single series only: the latest price.
    if (series.length === 1 && series[0].steps.length) {
      const s = series[0];
      const last = s.steps[s.steps.length - 1];
      svg.append(h('svg:text', { class: 'end-label', x: width - M.right, y: y(last.amount) - 8, 'text-anchor': 'end', text: fmt.money(last.amount, currency) }));
    }

    const cross = h('svg:line', { class: 'crosshair', y1: M.top, y2: M.top + ih, x1: 0, x2: 0, visibility: 'hidden' });
    svg.append(cross);
    const hit = h('svg:rect', { class: 'hit', x: M.left, y: M.top, width: iw, height: ih });
    svg.append(hit);
    plot.replaceChildren(svg, tooltip);

    const show = (index) => {
      if (index == null) {
        cross.setAttribute('visibility', 'hidden');
        tooltip.hidden = true;
        return;
      }
      cross.setAttribute('visibility', 'visible');
      focusIndex = index;
      const t = positions[index];
      const cx = x(t);
      cross.setAttribute('x1', cx);
      cross.setAttribute('x2', cx);
      tooltip.replaceChildren(
        h('p', { class: 'tt-date', text: fmt.date(t) }),
        ...series.map((s) => {
          const obs = s.observations.find((p) => p.ts === t);
          const v = obs ? obs.amount : valueAt(s, t);
          return h('p', { class: 'tt-row' },
            h('span', { class: `key-line series-${s.slot}`, 'aria-hidden': 'true' }),
            h('strong', { text: v == null ? '–' : fmt.money(v, currency) }),
            ' ',
            h('bdi', { class: 'tt-label', text: s.label }),
            obs ? h('span', { class: 'muted', text: ' observed' }) : null,
          );
        }),
      );
      tooltip.hidden = false;
      const left = Math.min(Math.max(0, cx + 12), width - 200);
      tooltip.style.left = `${cx > width * 0.6 ? Math.max(0, cx - 212) : left}px`;
    };
    const nearest = (px) => {
      let best = 0;
      let bestD = Infinity;
      positions.forEach((t, i) => {
        const d = Math.abs(x(t) - px);
        if (d < bestD) {
          bestD = d;
          best = i;
        }
      });
      return best;
    };
    hit.addEventListener('pointermove', (ev) => {
      const rect = svg.getBoundingClientRect();
      show(nearest(((ev.clientX - rect.left) / rect.width) * width));
    });
    hit.addEventListener('pointerleave', () => {
      if (document.activeElement !== plot) show(null);
    });
    plot.onkeydown = (ev) => {
      if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(ev.key)) return;
      ev.preventDefault();
      let i = focusIndex ?? (ev.key === 'ArrowLeft' ? positions.length : -1);
      if (ev.key === 'ArrowLeft') i = Math.max(0, i - 1);
      if (ev.key === 'ArrowRight') i = Math.min(positions.length - 1, i + 1);
      if (ev.key === 'Home') i = 0;
      if (ev.key === 'End') i = positions.length - 1;
      show(i);
    };
    plot.onfocus = () => show(focusIndex ?? positions.length - 1);
    plot.onblur = () => show(null);
    if (focusIndex != null && document.activeElement === plot) show(focusIndex);
  };

  plot.tabIndex = 0;
  plot.setAttribute('aria-label', `Price chart in ${currency}. Use left and right arrow keys to read prices by date.`);

  // Draw after insertion so the width is known; redraw on resize.
  requestAnimationFrame(draw);
  if (typeof ResizeObserver === 'function') {
    let last = 0;
    const ro = new ResizeObserver(() => {
      const w = Math.floor(plot.clientWidth);
      if (w && Math.abs(w - last) > 4) {
        last = w;
        draw();
      }
    });
    ro.observe(plot);
  }

  if (omitted > 0) root.append(h('p', { class: 'hint', text: `${fmt.plural(omitted, 'more listing')} not charted. The table includes them.` }));

  // Table view: every point, so no value depends on hover.
  const rows = listings
    .flatMap((l) => l.price_history.filter((p) => p.currency === currency).map((p) => ({ l, p })))
    .sort((a, b) => b.p.ts - a.p.ts || (a.p.record < b.p.record ? -1 : 1));
  root.append(
    h('details', { class: 'chart-table' },
      h('summary', {}, `Show all ${rows.length} prices as a table`),
      h('div', { class: 'table-scroll' },
        h('table', {},
          h('thead', {}, h('tr', {}, h('th', { scope: 'col', text: 'Date' }), h('th', { scope: 'col', text: 'Seller' }), h('th', { scope: 'col', class: 'num', text: 'Price' }), h('th', { scope: 'col', text: 'Source' }), h('th', { scope: 'col', text: 'Record' }))),
          h('tbody', {},
            rows.map(({ l, p }) =>
              h('tr', {},
                h('td', { text: fmt.date(p.ts) }),
                h('td', {}, h('bdi', { text: l.seller_name || fmt.shortHex(l.seller) })),
                h('td', { class: 'num', text: fmt.money(p.amount, p.currency) }),
                h('td', { text: p.source === 'seller' ? 'Seller' : 'Observation' }),
                h('td', {}, h('a', { href: `#/data/${p.record}`, class: 'record-link' }, h('code', { class: 'hex', text: fmt.shortHex(p.record) }))),
              ),
            ),
          ),
        ),
      ),
    ),
  );
  return root;
}
