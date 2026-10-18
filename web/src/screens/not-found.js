import { h } from '../dom.js';

export default {
  id: 'not-found',
  render(ctx) {
    return h('section', { class: 'screen' },
      h('h1', { text: 'Page not found' }),
      h('p', {}, 'No screen matches ', h('code', { text: ctx.path }), '. ', h('a', { href: '#/search', text: 'Go to search' }), '.'),
    );
  },
};
