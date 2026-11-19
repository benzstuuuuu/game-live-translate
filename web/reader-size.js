// /reader text size (FR-WEB-06, #91): a 70–250% slider with A−/A+ in the
// control sheet, scaling every line from the medium sizes. Remembered in this
// browser only. Storage can be blocked (private mode); the page still works
// at 100%. Old saved picks (s/m/l, #45) map to 75/100/130.
(() => {
  const KEY = 'yokoten.reader.size';
  const MIN = 70, MAX = 250, STEP = 5, DEFAULT = 100;
  const LEGACY = { s: 75, m: 100, l: 130 };
  const el = (id) => document.getElementById(id);
  const range = el('size');

  const clamp = (pct) => Math.min(MAX, Math.max(MIN, Math.round(pct / STEP) * STEP));

  function apply(pct) {
    el('page').style.setProperty('--scale', String(pct / 100));
    range.value = String(pct);
    range.setAttribute('aria-valuetext', `${pct}%`);
    el('size-value').textContent = `${pct}%`;
    el('size-down').disabled = pct <= MIN;
    el('size-up').disabled = pct >= MAX;
  }

  function pick(pct) {
    apply(pct);
    try { localStorage.setItem(KEY, String(pct)); } catch { /* not remembered */ }
  }

  range.addEventListener('input', () => pick(clamp(Number(range.value))));
  el('size-down').addEventListener('click', () => pick(clamp(Number(range.value) - STEP)));
  el('size-up').addEventListener('click', () => pick(clamp(Number(range.value) + STEP)));

  let saved = null;
  try { saved = localStorage.getItem(KEY); } catch { /* blocked */ }
  if (Object.hasOwn(LEGACY, saved)) pick(LEGACY[saved]);
  else apply(/^\d+$/.test(saved) ? clamp(Number(saved)) : DEFAULT);
})();
