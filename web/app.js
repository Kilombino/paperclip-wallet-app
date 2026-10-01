'use strict';
let token = '';
let network = null;
let uncertainMutation = false;
let lightningEnabled = false;
let setupPending = false;
const $ = id => document.getElementById(id);
const status = text => { $('status').textContent = text; };
async function api(path, body) {
  const response = await fetch('/api/v1/' + path, {
    method: body === undefined ? 'GET' : 'POST', cache: 'no-store', redirect: 'error',
    headers: {Authorization: 'Bearer ' + token, 'Content-Type': 'application/json'},
    ...(body === undefined ? {} : {body: JSON.stringify(body)})
  });
  if (!response.ok) {
    let message = '';
    try { const detail = await response.json(); message = typeof detail.message === 'string' ? detail.message.slice(0, 900) : ''; } catch {}
    if (message.includes('no pending lightning receive')) message = 'No received payment matches this invoice in this wallet. An invoice from another wallet belongs in Pay someone, not Received status.';
    if (message.includes('dust') || message.includes('funded-HTLC minimum')) message = 'Payment cannot be constructed: the HTLC or remaining change is too small after recovery reserves. Check your Ark balance and try a larger payment or refresh eligible inputs.';
    throw new Error(message || 'Wallet request failed (' + response.status + '). Check wallet access and local services.');
  }
  return response.status === 204 ? null : response.json();
}
async function update() {
  const [balance, chain, vtxos, exits] = await Promise.all([api('wallet/balance'), api('onchain/balance'), api('wallet/vtxos'), api('exits/status/all')]);
  $('exits').textContent = JSON.stringify(exits, null, 2);
  $('ark-balance').textContent = Number.isSafeInteger(balance.spendable_sat) ? balance.spendable_sat.toLocaleString() : 'See details';
  $('chain-balance').textContent = Number.isSafeInteger(chain.confirmed_sat) ? chain.confirmed_sat.toLocaleString() : 'See details';
  $('vtxos').textContent = JSON.stringify({balance, onchain: chain, vtxos}, null, 2);
}
async function run(button, operation) {
  button.disabled = true;
  try { await operation(); } catch (error) { status(error.message); }
  finally { button.disabled = false; }
}
function accessToken(value) {
  // Umbrel supplies a random 32-byte app password, never a wallet seed.
  if (/^[0-9a-f]{64}$/i.test(value)) {
    return btoa(String.fromCharCode(0, ...value.match(/../g).map(hex => parseInt(hex, 16))))
      .replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
  }
  return value;
}
async function connectWallet() {
    const info = await api('wallet/identity');
    if (!['bitcoin', 'regtest'].includes(info.network) || info.exit_profile !== 2) throw new Error('Unsupported network or ASP recovery profile.');
    network = info.network;
    lightningEnabled = info.lightning_enabled === true;
    $('ln-controls').disabled = !lightningEnabled;
    $('ln-capability').textContent = lightningEnabled ? 'Experimental XBT Lightning enabled. Channel and Ark pool liquidity are required. Recovery reserves are included in your payment cost.' : 'This ASP has not enabled funded Lightning. Payment controls are unavailable.';
    $('network').textContent = network === 'bitcoin' ? 'XBT MAINNET · EXPERIMENTAL' : 'XBT REGTEST';
    await update();
    $('setup').hidden = true; $('login').hidden = true; $('wallet').hidden = false;
    status('Wallet connected. Verify the displayed network before funding.');
}
$('unlock').addEventListener('submit', event => { event.preventDefault(); run(event.submitter, async () => {
  token = accessToken($('token').value.trim()); $('token').value = '';
  try {
    const existing = await api('wallet');
    if (existing.fingerprint !== null && typeof existing.fingerprint !== 'string') throw new Error('Unrecognized wallet status.');
    setupPending = false;
    if (existing.fingerprint === null) {
      $('login').hidden = true; $('setup').hidden = false;
      status('Choose your XBT backend and ASP to create a wallet.');
      return;
    }
    await connectWallet();
  } catch (error) { token = ''; throw error; }
}); });
function endpoint(id, protocols) {
  const value = $(id).value.trim();
  const url = new URL(value);
  if (!protocols.includes(url.protocol) || url.username || url.password || url.hash || url.search) {
    throw new Error('Use an HTTP(S) endpoint without embedded credentials, query, or fragment.');
  }
  return value;
}
$('create-wallet').onsubmit = event => { event.preventDefault(); run(event.submitter, async () => {
  if (!token || setupPending) throw new Error('Lock and unlock to check whether the previous setup completed before retrying.');
  const selected = $('setup-network').value;
  if (!['mainnet', 'regtest'].includes(selected) || !$('setup-ack').checked) throw new Error('Choose the network and confirm the backup requirement.');
  const body = {
    network: selected, ark_server: endpoint('setup-asp', ['http:', 'https:']),
    chain_source: {bitcoind: {
      bitcoind: endpoint('setup-rpc', ['http:', 'https:']),
      bitcoind_auth: {'user-pass': {user: $('setup-user').value, pass: $('setup-password').value}}
    }}, force: false
  };
  if (!body.chain_source.bitcoind.bitcoind_auth['user-pass'].user || !body.chain_source.bitcoind.bitcoind_auth['user-pass'].pass) throw new Error('RPC credentials are required.');
  if (!confirm('Create a new wallet on XBT ' + selected + '? For an existing wallet, cancel and restore its complete backup instead.')) return;
  setupPending = true;
  $('setup-password').value = '';
  await api('wallet/create', body);
  setupPending = false;
  await connectWallet();
}); };
$('setup-lock').onclick = () => {
  token = ''; $('setup-password').value = ''; $('setup').hidden = true; $('login').hidden = false;
  status('Setup locked. Unlock to check wallet state.');
};
$('lock').onclick = () => { token = ''; network = null; $('network').textContent = 'NETWORK UNVERIFIED'; $('deposit-address').textContent = ''; $('exits').textContent = '';  $('wallet').hidden = true; $('login').hidden = false; $('vtxos').textContent = ''; $('address').textContent = ''; $('destination').value = ''; status('Wallet locked.'); };
$('reload').onclick = event => run(event.target, async () => { await update(); status('Balances updated.'); });
$('receive').onclick = event => run(event.target, async () => { const result = await api('wallet/addresses/next', {}); $('address').textContent = result.address; status('New Ark receive address.'); });
$('send').addEventListener('submit', event => { event.preventDefault(); run(event.submitter, async () => {
  const destination = $('destination').value.trim(), amount = Number($('amount').value);
  if (!Number.isSafeInteger(amount) || amount <= 0) throw new Error('Enter a positive whole number of sats.');
  if (!network) throw new Error('Unlock and verify the wallet network first.');
  if (!confirm('Send ' + amount.toLocaleString() + ' sats to:\n' + destination + '\n\nServer fees may apply.')) return;
  status('Submitting once. If the connection fails, check history before trying again.');
  await mutate('wallet/send', {destination, amount_sat: amount});
  $('destination').value = ''; $('amount').value = ''; await update(); status('Transfer completed.');
}); });
$('refresh').onclick = event => run(event.target, async () => {
  if (!confirm('Refresh all eligible VTXOs? Transaction fees may apply.')) return;
  await mutate('wallet/refresh/all', {}); await update(); status('Refresh requested. Wait for confirmation and update to check completion.');
});

// Do not automatically retry an operation after an ambiguous transport failure.
async function mutate(path, body) {
  if (!token || !network) throw new Error('Unlock and verify the wallet network first.');
  if (uncertainMutation) throw new Error('A prior submission needs checking. Review wallet history with the CLI before reloading and retrying.');
  uncertainMutation = true;
  const result = await api(path, body);
  uncertainMutation = false;
  return result;
}

function sats(id, optional = false) {
  const value = $(id).value.trim();
  if (optional && value === '') return null;
  if (!/^\d+$/.test(value) || !Number.isSafeInteger(Number(value)) || Number(value) <= 0) {
    throw new Error('Enter a positive whole number of sats.');
  }
  return Number(value);
}
$('chain-send').onsubmit = event => { event.preventDefault(); run(event.submitter, async () => {
  const destination = $('chain-destination').value.trim(), amount = sats('chain-amount');
  if (!destination) throw new Error('Enter an XBT address.');
  if (!confirm('Send ' + amount.toLocaleString() + ' sats on ' + network + ' to:\n' + destination + '\n\nMiner fees are additional.')) return;
  const result = await mutate('onchain/send', {destination, amount_sat: amount});
  $('chain-result').textContent = result.txid;
  $('chain-destination').value = ''; $('chain-amount').value = '';
  status('Transaction submitted. Wait for confirmation; do not send again.');
}); };
$('ln-pay').onsubmit = event => { event.preventDefault(); run(event.submitter, async () => {
  if (!lightningEnabled) throw new Error('Lightning is not enabled in this wallet recovery profile.');
  const destination = $('ln-destination').value.trim(), amount = sats('ln-amount', true);
  if (!destination) throw new Error('Enter an XBT Lightning request.');
  if (!confirm('Pay ' + (amount === null ? 'the invoice amount' : amount.toLocaleString() + ' sats') + ' from Ark on ' + network + '?\n' + destination + '\n\nService fees plus 4,000–6,000 sats of recovery reserves per input apply. Failed payments may also consume refund reserves.')) return;
  const result = await mutate('lightning/pay', {destination, amount_sat: amount, comment: null});
  $('ln-result').textContent = JSON.stringify(result, null, 2);
  paymentSummary('Submitted', 'Payment submitted once. Check Sent status before trying again.');
  if (result.payment_hash) { $('ln-identifier').value = result.payment_hash; $('ln-direction').value = 'sends'; }
  $('ln-destination').value = ''; $('ln-amount').value = '';
  status('Payment submitted. Check its status before making another payment.');
}); };
$('ln-receive').onsubmit = event => { event.preventDefault(); run(event.submitter, async () => {
  if (!lightningEnabled) throw new Error('Lightning is not enabled in this wallet recovery profile.');
  const amount = sats('ln-receive-amount');
  const result = await mutate('lightning/receives/invoice', {
    amount_sat: amount, description: $('ln-description').value.trim() || null, token: null
  });
  $('ln-invoice').textContent = result.invoice; $('ln-copy').hidden = false;
  paymentSummary('Awaiting payment', 'Share this invoice. Keep the wallet online until settlement completes.');
  $('ln-identifier').value = result.invoice; $('ln-direction').value = 'receives';
  status('Invoice created. A payment is not settled until the wallet reports completion.');
}); };
$('ln-check').onsubmit = event => { event.preventDefault(); run(event.submitter, async () => {
  const identifier = $('ln-identifier').value.trim(), direction = $('ln-direction').value;
  if (!identifier || !['sends', 'receives'].includes(direction)) throw new Error('Choose a payment and direction.');
  const result = await api('lightning/' + direction + '/' + encodeURIComponent(identifier));
  $('ln-result').textContent = JSON.stringify(result, null, 2);
  const state = typeof result.state === 'string' ? result.state : 'See details';
  paymentSummary(state, state === 'unknown' ? 'No outgoing payment is recorded for this identifier. Checking status does not pay an invoice.' : 'Status from your wallet. Review the details before submitting any further payment.');
  status('Payment status updated.');
}); };
$('history-load').onclick = event => run(event.target, async () => {
  const [ark, onchain, receives] = await Promise.all([
    api('history'), api('onchain/transactions'), api('lightning/receives')
  ]);
  $('history').textContent = JSON.stringify({ark, onchain, lightning_receives: receives}, null, 2);
  renderActivity(ark);
  status('Activity updated.');
});
const lockSession = $('lock').onclick;
$('lock').onclick = () => {
  lockSession();
  lightningEnabled = false; $('ln-controls').disabled = true;
  for (const id of ['ln-invoice', 'ln-result', 'chain-result', 'history', 'ark-balance', 'chain-balance']) $(id).textContent = '';
  for (const id of ['ln-destination', 'ln-amount', 'ln-receive-amount', 'ln-description', 'ln-identifier', 'chain-destination', 'chain-amount']) $(id).value = '';
};
$('deposit').onclick = event => run(event.target, async () => {
  const result = await api('onchain/addresses/next', {});
  $('deposit-address').textContent = result.address;
  status('Deposit address for ' + (network === 'bitcoin' ? 'XBT mainnet' : 'regtest') + '.');
});
$('board').onsubmit = event => { event.preventDefault(); run(event.submitter, async () => {
  const amount = Number($('board-amount').value);
  if (!Number.isSafeInteger(amount) || amount < 20000) throw new Error('Enter at least 20,000 whole sats.');
  if (!confirm('Board ' + amount.toLocaleString() + ' sats on ' + network + '? Recovery reserves and miner fees apply.')) return;
  await mutate('boards/board-amount', {amount_sat: amount}); await update();
  status('Board submitted. Wait for confirmations; do not submit again.');
}); };
$('withdraw').onclick = event => run(event.target, async () => {
  if (!confirm('Withdraw all Ark funds to your on-chain wallet? Fees apply.')) return;
  await mutate('wallet/offboard/all', {}); await update(); status('Withdrawal submitted.');
});
$('exit-start').onclick = event => run(event.target, async () => {
  if (!confirm('Start emergency recovery of all Ark funds? This requires on-chain fees and a timelock. Continue only after backing up your complete wallet.')) return;
  await mutate('exits/start/all', {}); await update(); status('Exit started. Progress recovery until funds are claimable.');
});
$('exit-progress').onclick = event => run(event.target, async () => {
  await mutate('exits/progress', {wait: false}); await update(); status('Recovery progress updated.');
});
$('exit-claim').onclick = event => run(event.target, async () => {
  if (!confirm('Claim all matured exits to your on-chain wallet? Miner fees apply.')) return;
  const address = await api('onchain/addresses/next', {});
  await mutate('exits/claim/all', {destination: address.address}); await update(); status('Claim submitted. Wait for confirmation.');
});

function paymentSummary(title, detail) {
  const box = $('payment-summary'); box.hidden = false;
  box.textContent = title.replaceAll('_', ' ') + ' — ' + detail;
}
$('ln-copy').onclick = event => run(event.target, async () => {
  await navigator.clipboard.writeText($('ln-invoice').textContent); status('Invoice copied.');
});
function renderActivity(records) {
  const list = $('activity-list'); list.replaceChildren();
  const rows = Array.isArray(records) ? records : [];
  $('activity-summary').textContent = rows.length + ' Ark movements · ' + rows.filter(r => r.status === 'pending').length + ' pending';
  if (!rows.length) { list.textContent = 'No Ark activity yet. Deposits and payments will appear here.'; return; }
  for (const row of rows) {
    const card = document.createElement('article'); card.className = 'activity-row';
    const amount = Number.isSafeInteger(row.effective_balance_sat) ? row.effective_balance_sat : null;
    const heading = document.createElement('strong');
    heading.textContent = (row.subsystem?.name || 'Ark') + ' · ' + (row.subsystem?.kind || 'Movement');
    const value = document.createElement('span'); value.className = amount > 0 ? 'positive' : 'amount';
    value.textContent = amount === null ? 'Amount unavailable' : (amount > 0 ? '+' : '') + amount.toLocaleString() + ' sats';
    const detail = document.createElement('p');
    const date = new Date(row.time?.created_at);
    detail.textContent = (row.status || 'Unknown') + ' · ' + (Number.isFinite(date.getTime()) ? date.toLocaleString() : 'Time unavailable') + ' · Fee: ' + (Number.isSafeInteger(row.offchain_fee_sat) ? row.offchain_fee_sat.toLocaleString() + ' sats' : 'unavailable');
    card.append(heading, value, detail); list.append(card);
  }
}
const reviewLock = $('lock').onclick;
$('lock').onclick = () => { reviewLock(); $('activity-list').replaceChildren(); $('activity-summary').textContent = ''; $('payment-summary').textContent = ''; $('payment-summary').hidden = true; $('ln-copy').hidden = true; };
