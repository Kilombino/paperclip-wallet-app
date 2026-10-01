import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import vm from 'node:vm';

const source = readFileSync(new URL('../web/app.js', import.meta.url), 'utf8');
const html = readFileSync(new URL('../web/index.html', import.meta.url), 'utf8');
const ids = new Set([...html.matchAll(/\bid="([^"]+)"/g)].map(match => match[1]));
function fixture(network, lightningEnabled = true, empty = false) {
  const elements = new Map(), calls = [];
  const element = id => {
    assert(ids.has(id), 'UI element must exist in the HTML: ' + id);
    if (!elements.has(id)) elements.set(id, {value: '', textContent: '', hidden: false,
      setAttribute() {}, replaceChildren() {}, append() {}, addEventListener(type, fn) { this[type] = fn; }});
    return elements.get(id);
  };
  let failPath = null;
  const context = vm.createContext({PaperclipReceive: {show() {}, clear() {}}, document: {getElementById: element, createElement: () => ({textContent:'',append() {}})}, confirm: () => true, URL, btoa, setInterval: () => {}, sessionStorage: { getItem: () => null, setItem() {}, removeItem() {} },
    fetch: async (path, options) => {
      calls.push({path, options});
      if (failPath && path.endsWith(failPath)) throw Error('Transport interrupted');
      const result = path.endsWith('/wallet') ? {fingerprint: empty ? null : 'test-fingerprint'}
        : path.endsWith('/wallet/identity') ? {network, exit_profile: 2, lightning_enabled: lightningEnabled}
        : path.endsWith('/fees/ark/send') ? {recipient_amount_sat: JSON.parse(options.body).amount_sat, recovery_reserve_sat: 6000, service_fee_sat: 0, total_debit_sat: JSON.parse(options.body).amount_sat + 6000, remaining_spendable_sat: 92000, input_count: 1, vtxos_spent: ['test-vtxo']}
        : path.endsWith('/wallet/balance') ? {spendable_sat: 100000}
        : path.endsWith('/onchain/balance') ? {confirmed_sat: 50000}
        : path.endsWith('/addresses/next') ? {address: 'bc1-test-address'}
        : path.endsWith('/lightning/pay') ? {payment_hash: 'test-hash', message: 'initiated'}
        : path.endsWith('/receives/invoice') ? {invoice: 'ln-test-invoice'}
        : path.endsWith('/onchain/message/sign') ? {...JSON.parse(options.body),scheme:'bip322-simple',signature:'smp-test-proof'}
        : path.endsWith('/onchain/message/verify') ? {valid:true}
        : path.endsWith('/onchain/send') ? {txid: 'test-txid'} : [];
      return {ok: true, status: 200, json: async () => result};
    }});
  vm.runInContext(source, context);
  async function trigger(id, kind = 'onclick') {
    const e = element(id);
    await e[kind]({preventDefault() {}, submitter: e, target: e});
    // Submit listeners intentionally return before run() completes.
    await new Promise(resolve => setImmediate(resolve));
  }
  return {context, element, calls, trigger, fail: (path = '/wallet/send') => { failPath = path; }};
}
for (const network of ['bitcoin', 'regtest']) {
  const f = fixture(network);
  f.element('token').value = 'local-test-token';
  await f.trigger('unlock', 'submit');
  assert.equal(f.element('wallet').hidden, false);
  assert.match(f.element('network').textContent, network === 'bitcoin' ? /MAINNET/ : /REGTEST/);
  assert(!f.calls.some(c => c.path.includes('ark-info')), 'Unlock must not require the ASP');
  f.element('board-amount').value = '25000';
  await f.trigger('board', 'onsubmit');
  assert.equal(JSON.parse(f.calls.find(c => c.path.endsWith('/board-amount')).options.body).amount_sat, 25000);
  await f.trigger('exit-start');
  await f.trigger('exit-progress');
  await f.trigger('exit-claim');
  assert(f.calls.some(c => c.path.endsWith('/exits/claim/all')));
  assert(f.calls.every(c => c.options.headers.Authorization === 'Bearer local-test-token'));
  f.element('chain-destination').value = 'bc1-test-address';
  f.element('chain-amount').value = '1234';
  await f.trigger('chain-send', 'onsubmit');
  assert.deepEqual(JSON.parse(f.calls.find(c => c.path.endsWith('/onchain/send')).options.body), {destination: 'bc1-test-address', amount_sat: 1234});
  assert.equal(f.element('chain-result').textContent, 'test-txid');
  f.element('ln-destination').value = 'lno-test-offer';
  f.element('ln-amount').value = '2000';
  await f.trigger('ln-pay', 'onsubmit');
  assert.deepEqual(JSON.parse(f.calls.find(c => c.path.endsWith('/lightning/pay')).options.body), {destination: 'lno-test-offer', amount_sat: 2000, comment: null});
  assert.equal(f.element('ln-identifier').value, 'test-hash');
  await f.trigger('ln-check', 'onsubmit');
  assert(f.calls.some(c => c.path.endsWith('/lightning/sends/test-hash')));
  f.element('ln-receive-amount').value = '3000';
  await f.trigger('ln-receive', 'onsubmit');
  assert.equal(f.element('ln-invoice').textContent, 'ln-test-invoice');
  await f.trigger('ln-check', 'onsubmit');
  assert(f.calls.some(c => c.path.endsWith('/lightning/receives/ln-test-invoice')));
  await f.trigger('history-load');
  assert(f.calls.some(c => c.path.endsWith('/onchain/transactions')));
  f.element('destination').value = 'ark1-recipient';
  f.element('amount').value = '2000';
  f.fail();
  await f.trigger('send', 'submit');
  await f.trigger('send', 'submit');
  assert.equal(f.calls.filter(c => c.path.endsWith('/wallet/send')).length, 1, 'Ambiguous payment must not be retried');
  await f.trigger('lock');
  assert.equal(f.element('network').textContent, 'NETWORK UNVERIFIED');
  assert.equal(f.element('ln-invoice').textContent, '');
  assert.equal(f.element('history').textContent, '');
}
for (const path of ['/lightning/pay', '/onchain/send']) {
  const f = fixture('regtest');
  f.element('token').value = 'test'; await f.trigger('unlock', 'submit');
  const prefix = path.includes('lightning') ? 'ln' : 'chain';
  const form = prefix === 'ln' ? 'ln-pay' : 'chain-send';
  f.element(prefix + '-destination').value = 'test-destination';
  for (const invalid of ['0', '-1', '0.5', '9007199254740992', '1e3']) {
    f.element(prefix + '-amount').value = invalid;
    await f.trigger(form, 'onsubmit');
  }
  assert(!f.calls.some(c => c.path.endsWith(path)));
  f.element(prefix + '-amount').value = '1000'; f.fail(path);
  await f.trigger(form, 'onsubmit'); await f.trigger(form, 'onsubmit');
  assert.equal(f.calls.filter(c => c.path.endsWith(path)).length, 1);
  f.element('ln-identifier').value = 'known-hash'; f.element('ln-direction').value = 'sends';
  await f.trigger('ln-check', 'onsubmit');
  assert(f.calls.some(c => c.path.endsWith('/sends/known-hash')), 'Status reads must remain available after ambiguous submission');
}
console.log('PASS: networks, offline unlock, recovery, on-chain/Lightning request contracts, status/history, validation, auth, lock, and no payment retries');

for (const enabled of [false, null, 'true']) {
  const f = fixture('regtest', enabled);
  f.element('token').value = 'test'; await f.trigger('unlock', 'submit');
  assert.equal(f.element('ln-controls').disabled, true);
  f.element('ln-destination').value = 'test'; f.element('ln-amount').value = '1000';
  f.element('ln-receive-amount').value = '1000';
  await f.trigger('ln-pay', 'onsubmit'); await f.trigger('ln-receive', 'onsubmit');
  assert(!f.calls.some(c => c.options.method === 'POST' && c.path.includes('/lightning/')));
}
console.log('PASS: Lightning controls fail closed without explicit wallet capability');

for (const interrupted of [false, true]) {
  const f = fixture('regtest', false, true);
  f.element('token').value = '12'.repeat(32);
  await f.trigger('unlock', 'submit');
  assert.equal(f.element('setup').hidden, false);
  assert(!f.calls.some(c => c.options.method === 'POST'), 'Unlock must not create a wallet');
  assert.equal(f.calls[0].options.headers.Authorization,
    'Bearer ' + Buffer.concat([Buffer.from([0]), Buffer.from('12'.repeat(32), 'hex')]).toString('base64url'));
  f.element('setup-network').value = 'regtest';
  f.element('setup-asp').value = 'http://asp:3535';
  f.element('setup-rpc').value = 'http://selected-knots:18443';
  f.element('setup-user').value = 'rpc-user';
  f.element('setup-password').value = 'test-password';
  f.element('setup-ack').checked = false;
  await f.trigger('create-wallet', 'onsubmit');
  assert(!f.calls.some(c => c.options.method === 'POST'));
  f.element('setup-ack').checked = true;
  if (interrupted) f.fail('/wallet/create');
  await f.trigger('create-wallet', 'onsubmit');
  const creation = f.calls.filter(c => c.path.endsWith('/wallet/create'));
  assert.equal(creation.length, 1);
  assert.deepEqual(JSON.parse(creation[0].options.body), {
    network: 'regtest', ark_server: 'http://asp:3535', force: false,
    chain_source: {bitcoind: {bitcoind: 'http://selected-knots:18443',
      bitcoind_auth: {'user-pass': {user: 'rpc-user', pass: 'test-password'}}}}
  });
  assert.equal(f.element('setup-password').value, '');
  if (interrupted) {
    await f.trigger('create-wallet', 'onsubmit');
    assert.equal(f.calls.filter(c => c.path.endsWith('/wallet/create')).length, 1);
    await f.trigger('setup-lock');
    assert.equal(f.element('setup').hidden, true);
  }
}
console.log('PASS: authenticated first-run setup, platform password, explicit backend, and no ambiguous creation retry');

{
  const f = fixture('bitcoin');
  vm.runInContext("paymentSummary('unknown', 'No payment recorded'); renderActivity([])", f.context);
  assert.match(f.element('payment-summary').textContent, /unknown/);
  assert.match(f.element('activity-list').textContent, /No Ark activity/);
  f.element('token').value = 'test-token'; await f.trigger('unlock', 'submit');
  await f.trigger('lock');
  assert.equal(f.element('payment-summary').hidden, true);
  assert.equal(f.element('activity-summary').textContent, '');
}
console.log('PASS: readable empty activity, unknown payment, and lock clears review state');

assert(!/[\u00c2\u00c3\ufffd]/u.test(source + html), 'Text assets must not contain mojibake');
assert(!/[^\x00-\x7f]/.test(source), 'Use Unicode escapes in JavaScript to avoid encoding conversion');
console.log('PASS: UI text encoding');

{
  const f=fixture('bitcoin');
  vm.runInContext("vtxoSnapshot={balance:{spendable_sat:15000,needs_refresh_sat:0},tip:1000,rows:[]};renderVtxos()",f.context);
  assert.match(f.element('vtxo-tip').textContent,/1,000/);
  vm.runInContext("vtxoSnapshot.tip=null;renderVtxos()",f.context);
  assert.match(f.element('vtxo-tip').textContent,/unavailable/);
}
console.log('PASS: VTXO chain height and unavailable-tip handling');

for (const mode of ['cancel', 'estimate-error', 'approve', 'malformed']) {
  const f = fixture('bitcoin');
  f.element('token').value = 'test-token'; await f.trigger('unlock', 'submit');
  f.element('destination').value = 'ark1-recipient'; f.element('amount').value = '2000';
  let confirmation = '';
  f.context.confirm = text => { confirmation = text; return mode !== 'cancel'; };
  if (mode === 'estimate-error') f.fail('/fees/ark/send');
  if (mode === 'malformed') {
    const fetch = f.context.fetch;
    f.context.fetch = async (path, options) => path.endsWith('/fees/ark/send')
      ? {ok:true, status:200, json:async () => ({recipient_amount_sat:2000, total_debit_sat:2000})} : fetch(path, options);
  }
  await f.trigger('send', 'submit');
  const sends = f.calls.filter(c => c.path.endsWith('/wallet/send'));
  assert.equal(sends.length, mode === 'approve' ? 1 : 0);
  if (mode === 'approve' || mode === 'cancel') {
    assert.match(confirmation, /Recovery reserve: 6,000 sats/);
    assert.match(confirmation, /Total balance reduction: 8,000 sats/);
    assert.match(confirmation, /not separately refundable/);
  }
  if (mode === 'approve') assert.equal(JSON.parse(sends[0].options.body).max_total_sat, 8000);
  if (mode === 'estimate-error') assert.equal(vm.runInContext('uncertainMutation', f.context), false);
}
console.log('PASS: Ark cost review, cancellation, failed or malformed estimates, and approved debit cap');

for (const change of ['destination', 'lock']) {
  const f = fixture('bitcoin');
  f.element('token').value = 'test-token'; await f.trigger('unlock', 'submit');
  f.element('destination').value = 'ark1-recipient'; f.element('amount').value = '2000';
  const fetch = f.context.fetch;
  f.context.fetch = async (path, options) => {
    const result = await fetch(path, options);
    if (path.endsWith('/fees/ark/send')) {
      if (change === 'lock') await f.trigger('lock');
      else f.element('destination').value = 'ark1-changed';
    }
    return result;
  };
  await f.trigger('send', 'submit');
  assert(!f.calls.some(c => c.path.endsWith('/wallet/send')), 'Stale estimate must not send');
}
console.log('PASS: edited destination and locked wallet invalidate an in-flight estimate');

for (const scenario of ['approve', 'cancel', 'lock', 'edit']) {
  const f = fixture('bitcoin');
  f.element('token').value = 'test-token'; await f.trigger('unlock', 'submit');
  f.element('message-address').value = 'bc1p-test';
  f.element('message-text').value = '  Paperclip challenge\nnonce: 123\n';
  let review = '';
  f.context.confirm = text => {review = text; return scenario !== 'cancel';};
  const fetch = f.context.fetch;
  f.context.fetch = async (path, options) => {
    const result = await fetch(path, options);
    if (path.endsWith('/onchain/message/sign')) {
      if (scenario === 'lock') await f.trigger('lock');
      if (scenario === 'edit') f.element('message-text').value = 'changed';
    }
    return result;
  };
  await f.trigger('onchain-message', 'onsubmit');
  assert(review.includes('  Paperclip challenge\nnonce: 123\n'));
  const signed = f.calls.filter(c => c.path.endsWith('/onchain/message/sign'));
  assert.equal(signed.length, scenario === 'cancel' ? 0 : 1);
  if (signed.length) assert.equal(JSON.parse(signed[0].options.body).message, '  Paperclip challenge\nnonce: 123\n');
  assert.equal(f.element('message-proof').hidden, scenario !== 'approve');
  if (scenario === 'approve') {
    f.element('message-verify-signature').value = 'smp-test-proof';
    await f.trigger('message-verify');
    assert.match(f.element('message-verification').textContent, /^Valid:/);
    await f.trigger('message-text', 'oninput');
    assert.equal(f.element('message-proof').hidden, true);
    await f.trigger('lock');
    assert.equal(f.element('message-text').value, '');
  }
  assert(!f.calls.some(c => /onchain\/(send|drain)|wallet\/send/.test(c.path)));
}
console.log('PASS: exact message review, cancellation, no payment, verification, edit/lock invalidation');
