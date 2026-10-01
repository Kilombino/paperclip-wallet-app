import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import vm from 'node:vm';
import {createRequire} from 'node:module';

// Optional independent decoder: node scripts/test-receive-qr.mjs /path/to/jsQR.js
const decode = process.argv[2] ? createRequire(import.meta.url)(process.argv[2]) : null;
const context = vm.createContext({window: {}});
vm.runInContext(readFileSync(new URL('../web/vendor/qrcode-generator-2.0.4.js', import.meta.url), 'utf8'), context);
vm.runInContext(readFileSync(new URL('../web/receive.js', import.meta.url), 'utf8'), context);
const {encode} = context.window.PaperclipReceive;
const invoice = readFileSync(new URL('../lib/testdata/bolt12-invoice.txt', import.meta.url), 'utf8').trim();
const bolt11 = readFileSync(new URL('../bark/src/actions/lightning/receive.rs', import.meta.url), 'utf8').match(/const TEST_INVOICE_STR: &str = "([^"]+)"/)[1];
const payloads = [
  bolt11,
  'bc1qcz522hnzdjzk8hm2ska5uvaqy5r3jk4kcr5npx',
  'tark1pwh9vsmezqqpharv69q4z8m6x364d5m5prnmcalcalq9pdmzw0y7mpveck4pcfhezqypczkrrj3lkx5ue4qrf4jc7ztpt9htdttmh2judhqnu7aue8p0y9mq47jn9z',
  'lno1' + 'q'.repeat(220), // Reusable offers use the same lossless encoding.
  invoice,
];
for (const payload of payloads) {
  const svg = encode(payload);
  const modules = Number(svg.match(/viewBox="0 0 (\d+)/)[1]);
  assert(svg.includes('fill="white"') && svg.includes('fill="black"'));
  assert(!svg.includes(payload), 'Payload must not be inserted as SVG markup');
  const scale = 4, width = modules * scale;
  const rgba = new Uint8ClampedArray(width * width * 4).fill(255);
  for (const [, x, y] of svg.matchAll(/M(\d+),(\d+)h1v1h-1z/g)) {
    assert(Number(x) >= 4 && Number(y) >= 4 && Number(x) < modules - 4 && Number(y) < modules - 4);
    for (let dy = 0; dy < scale; dy++) for (let dx = 0; dx < scale; dx++) {
      const offset = ((Number(y) * scale + dy) * width + Number(x) * scale + dx) * 4;
      rgba[offset] = rgba[offset + 1] = rgba[offset + 2] = 0;
    }
  }
  if (decode) assert.equal(decode(rgba, width, width)?.data, payload, 'Scanned payload must match exactly');
}
for (const invalid of ['', null, '<svg>\n', 'x'.repeat(4097), '\u00e9']) assert.throws(() => encode(invalid));
console.log('PASS: local QR encoding, four-module quiet zone, no payload markup, bounded inputs' + (decode ? ', independent scan round trips' : ''));
