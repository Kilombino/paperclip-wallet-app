/* Receive requests stay in this browser. Never send them to a QR service. */
(() => {
  'use strict';
  const cards = new Map();
  const node = (tag, text, className) => {
    const element = document.createElement(tag);
    if (text) element.textContent = text;
    if (className) element.className = className;
    return element;
  };

  function encode(payload) {
    if (typeof payload !== 'string' || !payload || payload.length > 4096 || /[^\x21-\x7e]/.test(payload)) {
      throw new Error('Invalid receive request');
    }
    const code = qrcode(0, 'M');
    code.addData(payload, 'Byte');
    code.make();
    const count = code.getModuleCount(), size = count + 8;
    let path = '';
    for (let row = 0; row < count; row++) {
      for (let column = 0; column < count; column++) {
        if (code.isDark(row, column)) path += 'M' + (column + 4) + ',' + (row + 4) + 'h1v1h-1z';
      }
    }
    // Only encoder-generated coordinates enter this SVG; the payload is never markup.
    return '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ' + size + ' ' + size + '" width="' + size * 6 + '" height="' + size * 6 + '" shape-rendering="crispEdges"><rect width="100%" height="100%" fill="white"/><path d="' + path + '" fill="black"/></svg>';
  }

  async function copy(payload) {
    if (navigator.clipboard && window.isSecureContext) {
      await navigator.clipboard.writeText(payload);
      return;
    }
    // LAN HTTP installs do not have the Clipboard API.
    const input = node('textarea');
    input.value = payload;
    input.className = 'receive-copy-buffer';
    input.readOnly = true;
    document.body.append(input);
    const focused = document.activeElement;
    try {
      input.select();
      if (!document.execCommand('copy')) throw new Error('Select and copy the address below.');
    } finally {
      input.remove();
      if (focused && focused.focus) focused.focus();
    }
  }

  function clear() {
    for (const {card, url} of cards.values()) {
      card.remove();
      if (url) URL.revokeObjectURL(url);
    }
    cards.clear();
  }

  function show(outputId, payload, label) {
    const output = document.getElementById(outputId);
    const previous = cards.get(outputId);
    if (previous) {
      previous.card.remove();
      if (previous.url) URL.revokeObjectURL(previous.url);
    }
    const card = node('div', null, 'receive-card');
    const caption = node('p', label, 'receive-caption');
    const notice = node('p', 'Scan with an XBT-compatible wallet.', 'hint');
    notice.setAttribute('role', 'status');
    card.append(caption);
    let url = null;
    try {
      url = URL.createObjectURL(new Blob([encode(payload)], {type: 'image/svg+xml'}));
      const qr = node('img');
      qr.src = url;
      qr.alt = label + ' QR code. The same request is available as text below.';
      qr.className = 'receive-qr';
      card.append(qr);
    } catch (_) {
      notice.textContent = 'QR unavailable for this request. Copy the full text below instead.';
    }
    const actions = node('div', null, 'receive-actions');
    const copyButton = node('button', 'Copy request', 'secondary');
    copyButton.type = 'button';
    copyButton.onclick = async () => {
      try { await copy(payload); notice.textContent = 'Copied. Ready to share.'; }
      catch (_) { notice.textContent = 'Select and copy the request text below.'; }
    };
    actions.append(copyButton);
    if (navigator.share && window.isSecureContext) {
      const shareButton = node('button', 'Share', 'secondary');
      shareButton.type = 'button';
      shareButton.onclick = async () => {
        try { await navigator.share({title: label, text: payload}); }
        catch (error) { if (error.name !== 'AbortError') notice.textContent = 'Sharing unavailable. Use Copy request.'; }
      };
      actions.append(shareButton);
    }
    if (url) {
      const download = node('a', 'Save QR', 'receive-save');
      download.href = url;
      download.download = 'paperclip-' + outputId + '-qr.svg';
      actions.append(download);
    }
    card.append(actions, notice);
    (output.closest('.receive-raw') || output).before(card);
    cards.set(outputId, {card, url});
  }

  window.PaperclipReceive = {show, clear, encode};
})();
