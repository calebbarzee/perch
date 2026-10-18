// Perch background worker. It holds no data and makes no network requests.
// - Opens the options page on install (first run, docs/ux.md 9.5).
// - Sets the toolbar button state per tab from the content script's count.
// - Handles the Alt+Shift+P command.

chrome.runtime.onInstalled.addListener((details) => {
  if (details.reason === 'install') chrome.runtime.openOptionsPage();
});

chrome.runtime.onMessage.addListener((msg, sender) => {
  if (msg?.type !== 'perch:detected' || !sender.tab?.id) return;
  const tabId = sender.tab.id;
  const n = Array.isArray(msg.gtins) ? msg.gtins.length : 0;
  if (n === 0) {
    chrome.action.setBadgeText({ tabId, text: '' });
    chrome.action.setTitle({ tabId, title: 'Perch: no product number found on this page.' });
    chrome.action.disable(tabId);
  } else {
    chrome.action.enable(tabId);
    chrome.action.setBadgeText({ tabId, text: n > 1 ? String(n) : '' });
    chrome.action.setTitle({ tabId, title: n > 1 ? `Perch: ${n} product numbers on this page. Choose one.` : 'Perch: show records for this product' });
  }
});

chrome.commands.onCommand.addListener(async (command, tab) => {
  if (command !== 'toggle-panel') return;
  const { inPage } = await chrome.storage.local.get({ inPage: false });
  if (inPage && tab?.id) {
    const res = await chrome.tabs.sendMessage(tab.id, { type: 'perch:toggle' }).catch(() => null);
    if (res?.toggled) return;
  }
  chrome.action.openPopup().catch(() => {});
});
