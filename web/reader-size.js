// /reader text size (FR-WEB-06, #45): three sizes in the control sheet,
// remembered in this browser only. Storage can be blocked (private mode);
// the page still works at the default size.
(() => {
  const KEY = 'yokoten.reader.size';
  const SIZES = ['s', 'm', 'l'];
  const el = (id) => document.getElementById(id);

  function apply(size) {
    el('page').dataset.size = size;
    el('sizes').querySelectorAll('[data-size-pick]').forEach((b) =>
      b.setAttribute('aria-pressed', String(b.dataset.sizePick === size)));
  }

  el('sizes').addEventListener('click', (e) => {
    const t = e.target.closest('[data-size-pick]');
    if (!t) return;
    try { localStorage.setItem(KEY, t.dataset.sizePick); } catch { /* not remembered */ }
    apply(t.dataset.sizePick);
  });

  let saved = null;
  try { saved = localStorage.getItem(KEY); } catch { /* blocked */ }
  apply(SIZES.includes(saved) ? saved : 'm');
})();
