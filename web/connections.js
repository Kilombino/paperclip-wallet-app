'use strict';
// Connection checks are read-only. They never switch a funded wallet's backend.
async function checkConnections() {
  const session = token;
  const targets = [
    ['connection-wallet', 'wallet/identity', () => 'Wallet service reachable'],
    ['connection-node', 'bitcoin/tip', result => Number.isSafeInteger(result?.tip_height) ? 'Node reachable · block ' + result.tip_height.toLocaleString() : 'Node response unavailable'],
    ['connection-asp', 'wallet/next-round', () => 'Ark server reachable']
  ];
  await Promise.all(targets.map(async ([id, path, describe]) => {
    $(id).textContent = 'Checking…';
    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), 10000);
    try {
      const result = await api(path, undefined, 'GET', controller.signal);
      if (token === session && token) $(id).textContent = describe(result);
    } catch {
      if (token === session && token) $(id).textContent = 'Check unavailable — retry or check your server';
    } finally { clearTimeout(timeout); }
  }));
}
$('connections-check').onclick = event => run(event.target, checkConnections);
$('settings-lock').onclick = () => $('lock').click();
new MutationObserver(() => {
  if (!$('wallet').hidden) return;
  for (const id of ['connection-wallet', 'connection-node', 'connection-asp']) $(id).textContent = 'Not checked';
}).observe($('wallet'), {attributes: true, attributeFilter: ['hidden']});
