// Navigation changes only the visible view, never the payment state.

function showView(view) {
  document.querySelectorAll('[data-screen]').forEach(panel => { panel.hidden = panel.dataset.screen !== view; });
  document.querySelectorAll('.view-nav [data-view]').forEach(button => button.setAttribute('aria-pressed', String(button.dataset.view === view)));
}
document.querySelectorAll('[data-view]').forEach(button => button.addEventListener('click', () => { showView(button.dataset.view); if (button.dataset.view === 'activity') $('history-load').click(); }));
new MutationObserver(() => {
  document.body.classList.toggle('session-open', !$('wallet').hidden);
  if ($('wallet').hidden) showView('overview');
  window.scrollTo({top: 0, behavior: 'instant'});
}).observe($('wallet'), {attributes: true, attributeFilter: ['hidden']});
