'use strict';
// A review never submits a payment. The caller owns the single submission.
window.PaperclipReview = (() => {
  let pending = null;
  const dialog = document.createElement('dialog');
  dialog.className = 'review-sheet';
  dialog.setAttribute('aria-labelledby', 'review-title');
  const title = document.createElement('h2'); title.id = 'review-title'; title.textContent = 'Review and confirm';
  const text = document.createElement('div'); text.className = 'review-content';
  const actions = document.createElement('div'); actions.className = 'review-actions';
  const cancel = document.createElement('button'); cancel.type = 'button'; cancel.className = 'secondary'; cancel.textContent = 'Go back';
  const approve = document.createElement('button'); approve.type = 'button'; approve.textContent = 'Confirm';
  actions.append(cancel, approve); dialog.append(title, text, actions); document.body.append(dialog);
  function finish(value) { const resolve = pending; pending = null; if (dialog.open) dialog.close(); text.textContent = ''; resolve?.(value); }
  cancel.onclick = () => finish(false);
  approve.onclick = () => finish(true);
  dialog.addEventListener('cancel', event => { event.preventDefault(); finish(false); });
  dialog.addEventListener('close', () => { if (pending) finish(false); });
  return { ask(message) {
    if (pending) return Promise.resolve(false);
    const session = token;
    text.textContent = message;
    return new Promise(resolve => {
      pending = value => resolve(value && session === token && !!token);
      dialog.showModal(); cancel.focus();
    });
  }, cancel: () => finish(false) };
})();
