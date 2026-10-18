// Deterministic mock dataset for the mock client. It covers every state the
// views can report: verified and unverified authors, domain verification,
// conflicting facts, price history, verified purchases and each failure
// reason, hidden listings and reviews, an appeal, a redacted review, a
// retracted review, a forked author, a revoked key, and a retired author.

import { sha256Hex } from './sha256.js';
import { canonicalJson } from './canonical.js';
import { mockAuthor, makeRecord, makeReceipt } from './mock-crypto.js';

const DAY = 86400000;
const T0 = Date.UTC(2026, 2, 2, 9, 0, 0); // 2 March 2026
const EXPIRES = Date.UTC(2028, 0, 1);

function gtin13(base12) {
  let sum = 0;
  for (let i = 0; i < 12; i++) sum += Number(base12[11 - i]) * (i % 2 === 0 ? 3 : 1);
  return base12 + String((10 - (sum % 10)) % 10);
}

export function productKeyFor(body, rootId) {
  if (body.gtin) return 'gtin:' + body.gtin.padStart(14, '0');
  const trim = (s) => s.replace(/^[\t\n\f\r ]+|[\t\n\f\r ]+$/g, '').replace(/[A-Z]/g, (c) => c.toLowerCase());
  const esc = (s) => s.replace(/%/g, '%25').replace(/:/g, '%3a');
  if (body.mpn) return 'mpn:' + esc(trim(body.brand)) + ':' + esc(trim(body.mpn));
  return 'rec:' + rootId;
}

const PRODUCTS = {
  kettle: { name: 'Kestrel K2 electric kettle 1.7 L', brand: 'Kestrel', category: 'appliances/kitchen', gtin: gtin13('400638133393'),
    attributes: { Capacity: '1.7 L', Power: '2200 W', Material: 'Stainless steel' },
    description: 'Cordless kettle with a 360-degree base, limescale filter, and automatic shut-off.' },
  earbuds: { name: 'Aster A40 wireless earbuds', brand: 'Aster', category: 'electronics/audio', gtin: gtin13('501234567890'),
    attributes: { 'Battery life': '7 h (28 h with case)', Bluetooth: '5.3', 'Water resistance': 'IPX4' } },
  backpack: { name: 'Norrland 65 trekking backpack', brand: 'Norrland', category: 'outdoor/bags', gtin: gtin13('735003412776'),
    attributes: { Volume: '65 L', Weight: '2.1 kg' } },
  coffee: { name: 'Fennick Pro 10-cup drip coffee maker', brand: 'Fennick', category: 'appliances/kitchen', gtin: gtin13('871234500017'),
    attributes: { Capacity: '1.25 L', 'Keep warm': '40 min' } },
  monitor: { name: 'Calder C27 27-inch 4K monitor', brand: 'Calder', category: 'electronics/displays', gtin: gtin13('471234567801'),
    attributes: { Resolution: '3840 x 2160', Panel: 'IPS', 'Refresh rate': '60 Hz' } },
  skillet: { name: 'Juniper 26 cm cast-iron skillet', brand: 'Juniper', category: 'home/cookware', gtin: gtin13('590123412345'),
    attributes: { Diameter: '26 cm', Weight: '2.4 kg' } },
  shoe: { name: 'Pell Ridge trail running shoe', brand: 'Pell', category: 'sports/footwear', mpn: 'PR-220',
    attributes: { Drop: '6 mm', Weight: '290 g (EU 42)' } },
  lamp: { name: 'Marlow LED desk lamp', brand: 'Marlow', category: 'home/lighting', gtin: gtin13('761234567890'),
    attributes: { 'Color temperature': '2700-6500 K', Power: '8 W' } },
  mixer: { name: 'Tamsin 5 L stand mixer', brand: 'Tamsin', category: 'appliances/kitchen', gtin: gtin13('380012345678'),
    attributes: { Bowl: '5 L', Power: '1000 W' } },
  vacuum: { name: 'Ostrava R3 robot vacuum', brand: 'Ostrava', category: 'appliances/cleaning', gtin: gtin13('859012345678'),
    attributes: { Runtime: '150 min', 'Dust bin': '0.45 L' } },
  board: { name: 'Handmade oak cutting board', brand: 'Woodshop Ilse', category: 'home/kitchen',
    attributes: { Size: '40 x 25 cm' }, description: 'Single piece of oak, oiled. Each board differs.' },
};

export function buildMockDataset() {
  const actors = new Map();
  const actor = (name) => {
    if (!actors.has(name)) {
      const secret = sha256Hex('perch-mock-seed\n' + name);
      actors.set(name, { name, secret, author: mockAuthor(secret), seq: 0, prev: null, lastTs: 0 });
    }
    return actors.get(name);
  };
  const A = (name) => actor(name).author;

  const events = [];
  const ev = (day, who, kind, body, opts = {}) => {
    events.push({ day, who, kind, body, opts, order: events.length });
  };

  // People and organizations.
  const attestor = 'attestor';
  const node = 'node';
  const sellers = {
    brightline: { name: 'Brightline Electronics', domain: 'brightline.example', verified: true, domainVerified: true },
    harbor: { name: 'Harbor Outfitters', domain: 'harbor-outfitters.example', verified: true, domainVerified: true },
    cedar: { name: 'Cedar Home Goods', domain: 'cedarhome.example', verified: true, domainVerified: false },
    quickdeals: { name: 'QuickDeals Online', domain: 'quickdeals.example', verified: false, domainVerified: false },
  };
  const verifiedBuyers = ['ana', 'ben', 'chloe', 'dev', 'emma', 'farid', 'grace', 'hiro', 'ines', 'jonas', 'mo', 'noor'];
  const unverifiedBuyers = ['kai', 'lena', 'omar', 'priya'];
  const displayNames = {
    ana: 'Ana Souza', ben: 'Ben Okafor', chloe: 'Chloe Martin', dev: 'Dev Patel', emma: 'Emma Lindqvist',
    farid: 'Farid Haddad', grace: 'Grace Kim', hiro: 'Hiro Tanaka', ines: 'Inès Moreau', jonas: 'Jonas Weber',
    kai: 'kai_reviews', lena: 'Lena', mo: 'Mo Rahman', noor: 'Noor Aziz', omar: 'deals4u', priya: 'Priya',
  };

  ev(0, attestor, 'profile', { name: 'Demo identity desk', role: 'attestor', url: 'https://identity.perch.example/', about: 'Checks government ID or business registration in a video call, then attests the key. Mock data.' });
  ev(0, node, 'profile', { name: 'Node operator, perch.example', role: 'node', url: 'https://perch.example/' });
  ev(0, 'crawler', 'profile', { name: 'Open price watch', role: 'crawler', about: 'Visits public product pages once a week and records the shown price.' });
  ev(0, 'oldbot', 'profile', { name: 'Price bot 2025', role: 'crawler' });
  ev(1, 'oldbot', 'retire', { successor: A('crawler') });

  for (const [id, s] of Object.entries(sellers)) {
    ev(1, id, 'profile', { name: s.name, role: 'seller', url: `https://${s.domain}/` });
    if (s.verified) ev(2, attestor, 'attest.identity', { subject: A(id), person: sha256Hex('person\n' + id), level: 'business', country: id === 'harbor' ? 'NO' : 'DE', expires: EXPIRES });
    if (s.domainVerified) ev(2, attestor, 'attest.domain', { subject: A(id), domain: s.domain, expires: EXPIRES });
  }
  for (const b of [...verifiedBuyers, ...unverifiedBuyers]) {
    ev(3, b, 'profile', { name: displayNames[b], role: 'buyer' });
  }
  verifiedBuyers.forEach((b, i) => {
    ev(4, attestor, 'attest.identity', { subject: A(b), person: sha256Hex('person\n' + b), level: 'person', country: ['DE', 'FR', 'SE', 'NL', 'ES', 'PL'][i % 6], expires: EXPIRES });
  });

  // Products.
  ev(5, 'brightline', 'product', { ...PRODUCTS.kettle, images: [{ url: 'https://img.brightline.example/k2-front.jpg' }] }, { name: 'p-kettle' });
  ev(5, 'brightline', 'product', PRODUCTS.earbuds, { name: 'p-earbuds' });
  ev(6, 'quickdeals', 'product', { ...PRODUCTS.earbuds, name: 'Aster A40 True Wireless Earbuds', images: [{ url: 'https://cdn.quickdeals.example/a40.png' }] });
  ev(5, 'harbor', 'product', PRODUCTS.backpack, { name: 'p-backpack' });
  ev(5, 'brightline', 'product', PRODUCTS.coffee);
  ev(5, 'brightline', 'product', PRODUCTS.monitor);
  ev(5, 'harbor', 'product', PRODUCTS.skillet);
  ev(5, 'harbor', 'product', PRODUCTS.shoe);
  ev(5, 'brightline', 'product', PRODUCTS.lamp);
  ev(6, 'cedar', 'product', PRODUCTS.mixer);
  ev(6, 'brightline', 'product', PRODUCTS.vacuum);
  ev(6, 'cedar', 'product', PRODUCTS.board, { name: 'p-board' });
  ev(7, 'cedar', 'product', { name: 'Fennick Pro 10 cup coffee maker', brand: 'Fennick', gtin: PRODUCTS.coffee.gtin, category: 'appliances/kitchen' });
  // A product record that verified people flag as counterfeit: hidden (product threshold 5).
  ev(8, 'quickdeals', 'product', { name: 'USB-C charger 65 W, certified original', brand: 'Volta', gtin: gtin13('690123456789'), category: 'electronics/chargers' }, { name: 'p-charger' });

  const keyOf = {};
  for (const [k, p] of Object.entries(PRODUCTS)) keyOf[k] = p.gtin || p.mpn ? productKeyFor(p, null) : null;

  // Listings with price changes. Prices are in cents.
  let listingN = 0;
  const listing = (seller, product, prices, opts = {}) => {
    const name = `l-${seller}-${product}`;
    const s = sellers[seller];
    const base = {
      product: keyOf[product],
      url: `https://${opts.host || 'www.' + s.domain}/p/${product}-${++listingN}`,
      condition: opts.condition || 'new',
      title: opts.title,
      ships_to: opts.ships_to || ['DE', 'AT', 'NL'],
    };
    prices.forEach(([day, amount, availability = 'in_stock'], i) => {
      const body = (r) => ({
        ...base,
        product: base.product ?? 'rec:' + r.root('p-board'),
        price: { amount, currency: opts.currency || 'EUR' },
        availability,
      });
      ev(day, seller, 'listing', body, i === 0 ? { name } : { replaces: name, name: `${name}#${i}` });
    });
    return name;
  };

  listing('brightline', 'kettle', [[8, 5999], [40, 5499], [95, 5999], [150, 4999]], { title: 'Kestrel K2 kettle, brushed steel' });
  listing('cedar', 'kettle', [[10, 5799], [80, 5299], [160, 5199]]);
  listing('brightline', 'earbuds', [[9, 12900], [70, 11900], [140, 10900]]);
  listing('quickdeals', 'earbuds', [[12, 4900], [60, 4500]], { title: 'Aster A40 earbuds ORIGINAL best price' });
  listing('harbor', 'backpack', [[9, 21900], [120, 19900], [170, 18900]]);
  listing('brightline', 'coffee', [[9, 8999], [100, 7999]]);
  listing('cedar', 'coffee', [[11, 8499], [130, 8299]]);
  listing('brightline', 'monitor', [[9, 37900], [50, 35900], [110, 34900], [180, 32900]]);
  listing('quickdeals', 'monitor', [[13, 29900], [90, 31900, 'out_of_stock']]);
  listing('harbor', 'skillet', [[9, 4500], [100, 3900]]);
  listing('cedar', 'skillet', [[12, 4200]]);
  listing('harbor', 'shoe', [[10, 13900], [90, 11900], [150, 9900]], { ships_to: ['NO', 'SE', 'DK'], currency: 'NOK' });
  listing('harbor', 'shoe', [[10, 1390], [150, 1190]], { host: 'eu.harbor-outfitters.example' });
  listing('brightline', 'lamp', [[10, 3999], [120, 3499]]);
  listing('cedar', 'mixer', [[12, 29900], [110, 27900], [175, 26900]]);
  listing('brightline', 'vacuum', [[10, 39900], [100, 34900], [170, 32900]]);
  listing('quickdeals', 'vacuum', [[14, 19900]]);
  listing('cedar', 'board', [[12, 6500], [140, 6900, 'preorder']]);

  // Observations from the crawler.
  const observe = (product, listingName, points, host) => {
    for (const [day, amount] of points) {
      ev(day, 'crawler', 'observation', (r) => ({
        product: keyOf[product],
        url: `https://${host}/p/${product}`,
        price: { amount, currency: 'EUR' },
        observed_at: T0 + day * DAY - 3 * 3600000,
        listing: r.root(listingName),
        availability: 'in_stock',
      }));
    }
  };
  observe('kettle', 'l-brightline-kettle', [[30, 5499], [60, 5299], [120, 5999], [165, 4999]], 'www.brightline.example');
  observe('earbuds', 'l-brightline-earbuds', [[45, 12900], [100, 11500], [160, 10900]], 'www.brightline.example');
  observe('monitor', 'l-brightline-monitor', [[30, 37900], [80, 34900], [140, 34900], [185, 32500]], 'www.brightline.example');
  observe('vacuum', 'l-brightline-vacuum', [[60, 37900], [130, 34900]], 'www.brightline.example');

  // Receipts are detached tokens; reviews embed them.
  const receipt = (seller, listingName, buyer, day) => (r) =>
    makeReceipt(actor(seller).secret, r.root(listingName), A(buyer), T0 + day * DAY, sha256Hex(`nonce\n${seller}\n${buyer}\n${listingName}`));
  const forgedReceipt = (listingName, buyer, day) => (r) => {
    const rc = makeReceipt(actor('priya').secret, r.root(listingName), A(buyer), T0 + day * DAY, sha256Hex('nonce-forged'));
    return rc;
  };

  const review = (day, who, product, rating, text, opts = {}) => {
    ev(day, who, 'review', (r) => {
      const body = { product: product === 'board' ? 'rec:' + r.root('p-board') : keyOf[product], rating, text };
      if (opts.title) body.title = opts.title;
      if (opts.listing) body.listing = r.root(opts.listing);
      if (opts.receipt) body.receipt = opts.receipt(r);
      return body;
    }, { name: opts.name, replaces: opts.replaces, stripBody: opts.stripBody });
  };

  // Kettle.
  review(30, 'ana', 'kettle', 5, 'Boils a full litre in about three minutes. The lid opens wide enough to clean inside.', { title: 'Fast and easy to clean', listing: 'l-brightline-kettle', receipt: receipt('brightline', 'l-brightline-kettle', 'ana', 25) });
  review(35, 'ben', 'kettle', 3, 'Works, but the handle gets warm near the top.', { name: 'r-ben-kettle-old' });
  review(90, 'ben', 'kettle', 4, 'After three months: still no rust, and the warm handle is less of an issue than I first thought.', { title: 'Update after three months' });
  review(40, 'farid', 'kettle', 4, 'Quiet compared to my old kettle. The water gauge is hard to read in low light.', { receipt: receipt('brightline', 'l-brightline-kettle', 'farid', 33) });
  review(42, 'kai', 'kettle', 5, 'Best kettle ever!!! Buy it now.', { receipt: receipt('cedar', 'l-cedar-kettle', 'kai', 38) });
  review(55, 'lena', 'kettle', 2, 'The limescale filter fell out after two weeks.', { receipt: receipt('cedar', 'l-cedar-coffee', 'lena', 50) });
  review(60, 'hiro', 'kettle', 4, 'Good value at the lower price. The cord is short (75 cm).', { title: 'Short cord' });
  review(62, 'mo', 'kettle', 1, 'Leaked from the base on day one.');
  review(70, 'emma', 'kettle', 3, 'Fine kettle. Shuts off a little late.', { name: 'r-emma-kettle', receipt: receipt('cedar', 'l-cedar-kettle', 'emma', 65) });
  review(100, 'emma', 'kettle', 4, 'Fine kettle. The late shut-off went away after descaling.', { replaces: 'r-emma-kettle', receipt: receipt('cedar', 'l-cedar-kettle', 'emma', 65) });
  review(110, 'ines', 'kettle', 5, 'Le bouton est solide et la base tourne bien. Très satisfaite.', { title: 'Très bien' });
  review(115, 'brightline', 'kettle', 5, 'Our best-selling kettle this year.');

  // Earbuds.
  review(20, 'chloe', 'earbuds', 4, 'Clear sound, the case is a bit large.', { name: 'r-chloe-earbuds' });
  ev(25, 'chloe', 'retract', (r) => ({ target: r.id('r-chloe-earbuds') }));
  review(40, 'grace', 'earbuds', 4, 'Battery lasts about 6.5 hours at medium volume.', { receipt: receipt('brightline', 'l-brightline-earbuds', 'grace', 35) });
  review(48, 'omar', 'earbuds', 5, 'Bought from QuickDeals, cheapest price, fast shipping.', { receipt: receipt('quickdeals', 'l-quickdeals-earbuds', 'omar', 44) });
  review(52, 'jonas', 'earbuds', 2, 'The pair I got from a discount shop had different firmware and no app support. Bought a second pair from Brightline and that one works.', { title: 'Watch out for fakes' });
  review(75, 'farid', 'earbuds', 4, 'Comfortable for long calls. Noise cancelling is average.');
  review(120, 'hiro', 'earbuds', 5, 'イヤホンの音質はとても良いです。ケースも小さくて便利です。');

  // Backpack and others.
  review(45, 'dev', 'backpack', 4, 'Hip belt fits well.', { name: 'r-dev-backpack' });
  review(50, 'dev', 'backpack', 4, 'Hip belt fits well. Call me on +49 151 0000000 if you want to buy mine.', { name: 'r-dev-backpack#1', replaces: 'r-dev-backpack', stripBody: true });
  review(60, 'emma', 'backpack', 5, 'Carried 14 kg for five days without shoulder pain.', { receipt: receipt('harbor', 'l-harbor-backpack', 'emma', 20) });
  review(65, 'grace', 'backpack', 4, 'Rain cover included. Side pockets are hard to reach.');
  review(80, 'ana', 'monitor', 4, 'Sharp text, but the stand only tilts.', { receipt: receipt('brightline', 'l-brightline-monitor', 'ana', 72) });
  review(85, 'priya', 'monitor', 5, 'Amazing monitor.', { receipt: forgedReceipt('l-brightline-monitor', 'priya', 80) });
  review(86, 'omar', 'monitor', 5, 'Visit quickdeals for the best deal!!! Best deal best deal best deal.', { name: 'r-omar-monitor' });
  review(88, 'jonas', 'monitor', 3, 'Backlight bleed in the lower left corner. Usable for office work.');
  review(70, 'ines', 'coffee', 4, 'Keeps coffee warm without burning it.', { receipt: receipt('cedar', 'l-cedar-coffee', 'ines', 60) });
  review(75, 'ben', 'coffee', 4, 'Simple and reliable.');
  review(70, 'hiro', 'skillet', 5, 'Seasoning held up after six weeks of daily use.', { receipt: receipt('harbor', 'l-harbor-skillet', 'hiro', 30) });
  review(72, 'chloe', 'shoe', 4, 'Good grip on wet rock. Runs half a size small.');
  review(74, 'dev', 'lamp', 5, 'The warm setting is pleasant for reading.');
  review(90, 'cedar', 'mixer', 5, 'We use this mixer in our own test kitchen.', { receipt: receipt('cedar', 'l-cedar-mixer', 'cedar', 88) });
  review(95, 'grace', 'mixer', 4, 'Strong motor, loud on the highest speed.', { receipt: receipt('cedar', 'l-cedar-mixer', 'grace', 85) });
  review(96, 'noor', 'vacuum', 4, 'Maps the flat well and avoids cables.');
  review(150, 'noor', 'vacuum', 1, 'Do not buy. Order from quickdeals instead.');
  review(100, 'jonas', 'board', 5, 'Heavy, flat, and the oil finish is even.');

  // Flags and moderation.
  const flag = (day, who, targetName, reason, note) =>
    ev(day, who, 'flag', (r) => ({ target: r.root(targetName), reason, ...(note ? { note } : {}) }));
  flag(54, 'jonas', 'l-quickdeals-earbuds', 'counterfeit', 'The unit I received had a different serial format.');
  flag(56, 'ana', 'l-quickdeals-earbuds', 'counterfeit');
  flag(58, 'grace', 'l-quickdeals-earbuds', 'fraud', 'Price is 60 percent below every other seller.');
  flag(59, 'kai', 'l-quickdeals-earbuds', 'counterfeit');
  ev(64, 'quickdeals', 'appeal', (r) => ({ target: r.root('l-quickdeals-earbuds'), note: 'Our units come from an authorized distributor in Poland. We can show the invoice to any attestor.' }));
  flag(90, 'chloe', 'r-omar-monitor', 'spam');
  flag(91, 'dev', 'r-omar-monitor', 'spam');
  flag(92, 'emma', 'r-omar-monitor', 'spam');
  flag(93, 'ben', 'r-omar-monitor', 'spam');
  flag(94, 'grace', 'r-omar-monitor', 'misleading');
  for (const [i, who] of ['ana', 'ben', 'chloe', 'dev', 'emma'].entries()) flag(30 + i, who, 'p-charger', 'counterfeit', i === 0 ? 'No certification mark; the brand does not exist in the registry.' : undefined);
  flag(100, 'ben', 'l-quickdeals-monitor', 'misleading', 'Sold as new; arrived with an open box.');
  flag(101, 'chloe', 'l-quickdeals-monitor', 'misleading');
  flag(102, 'dev', 'l-quickdeals-monitor', 'fraud');
  flag(120, 'hiro', 'l-quickdeals-vacuum', 'misleading', 'Listing photo shows the R5 model.');
  flag(121, 'ines', 'l-quickdeals-vacuum', 'misleading');
  flag(122, 'farid', 'l-quickdeals-vacuum', 'counterfeit');
  flag(123, 'ben', 'l-cedar-skillet', 'misleading', 'Weight is 2.1 kg, not 2.4 kg.');

  // Redaction notice for Dev's review, published by the node operator.
  ev(55, node, 'redact', (r) => ({ target: r.id('r-dev-backpack#1'), reason: 'personal_data', note: 'The review contained a phone number.' }));

  // Noor's key was reported stolen after seq 4 (profile, review, ...).
  ev(160, attestor, 'attest.revoke', { subject: A('noor'), valid_through_seq: 2 });

  // Mo signs two different records at the same position: a fork.
  ev(130, 'mo', 'review', { product: keyOf.backpack, rating: 5, text: 'Great backpack.' }, { forkWith: { product: keyOf.backpack, rating: 1, text: 'Terrible backpack.' } });

  // Resolve events in time order.
  events.sort((a, b) => a.day - b.day || a.order - b.order);
  const ids = new Map();
  const roots = new Map();
  const r = {
    id: (name) => {
      if (!ids.has(name)) throw new Error(`mock data: unknown event ${name}`);
      return ids.get(name);
    },
    root: (name) => {
      if (!roots.has(name)) throw new Error(`mock data: unknown event ${name}`);
      return roots.get(name);
    },
  };
  const records = [];
  for (const e of events) {
    const a = actor(e.who);
    const ts = Math.max(T0 + e.day * DAY + e.order * 1000, a.lastTs);
    const body = typeof e.body === 'function' ? e.body(r) : e.body;
    const replaces = e.opts.replaces ? r.id(e.opts.replaces) : undefined;
    const { id, record } = makeRecord(a.secret, { seq: a.seq + 1, prev: a.prev, ts, kind: e.kind, replaces, body });
    if (e.opts.forkWith) {
      const alt = makeRecord(a.secret, { seq: a.seq + 1, prev: a.prev, ts: ts + 1000, kind: e.kind, body: e.opts.forkWith });
      records.push(record, alt.record);
    } else if (e.opts.stripBody) {
      const { body: _omit, ...header } = record;
      records.push(header);
    } else {
      records.push(record);
    }
    a.seq += 1;
    a.prev = id;
    a.lastTs = ts;
    if (e.opts.name) {
      ids.set(e.opts.name, id);
      roots.set(e.opts.name, e.opts.replaces ? r.root(e.opts.replaces) : id);
    }
  }

  const policy = {
    now: null,
    trusted_attestors: [{ author: A(attestor), kinds: ['attest.domain', 'attest.identity', 'attest.revoke'], not_after_seq: null }],
    flag_threshold: 3,
    flag_thresholds: { review: 5, product: 5 },
    flags_require_verified: true,
    seller_hidden_listing_limit: 3,
    seller_min_distinct_flaggers: 9,
    verified_purchase_requires_verified_buyer: true,
    price_requires_domain_verified: true,
    future_ts_tolerance_ms: 600000,
  };

  const authors = Object.fromEntries([...actors.values()].map((x) => [x.name, x.author]));
  return { records, policy, authors, keys: keyOf, canonicalSize: records.reduce((n, x) => n + canonicalJson(x).length + 1, 0) };
}
