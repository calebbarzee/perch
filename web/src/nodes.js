// Node actions shared by screens (ARCHITECTURE 10): stop using a node, and
// retry a node whose head claim for the user's log is unproven.

import { h, announce } from './dom.js';
import { forgetNode } from './settings.js';

export function hostOf(url) {
  try {
    return new URL(url).host;
  } catch {
    return url;
  }
}

// Disconnects and removes the node from the saved or session node list.
export async function stopUsingNode(client, url) {
  await client.disconnect(url);
  forgetNode(url);
  announce(`Stopped using ${hostOf(url)}.`);
}

// Gives the node another 60 s to send the records it claims.
export function retryNode(client, url) {
  if (typeof client.retryNode === 'function') client.retryNode(url);
  announce(`Asked ${hostOf(url)} again. Publishing stays paused until its records arrive.`);
}

// "Retry" and "Stop using this node" for a node with an unproven claim.
export function claimActions(client, url, after) {
  const host = hostOf(url);
  return h('p', { class: 'actions claim-actions', dataset: { url } },
    h('button', {
      type: 'button',
      class: 'button',
      'aria-label': `Retry ${host}`,
      dataset: { action: 'retry' },
      on: {
        click: () => {
          retryNode(client, url);
          after?.();
        },
      },
    }, 'Retry'),
    ' ',
    h('button', {
      type: 'button',
      class: 'button danger',
      'aria-label': `Stop using this node: ${host}`,
      dataset: { action: 'stop' },
      on: {
        click: async () => {
          await stopUsingNode(client, url);
          after?.();
        },
      },
    }, 'Stop using this node'),
  );
}

// Nodes whose claims need the user's action.
export function unprovenNodes(client) {
  const st = typeof client.publishStatus === 'function' ? client.publishStatus() : null;
  return (st?.nodes || []).filter((n) => n.unproven);
}
