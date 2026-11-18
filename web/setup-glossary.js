// /setup glossary section (#44 · FR-GLS-01, 03): the terms of the game in use
// and the shared ones. Add, edit (Enter or leaving a box) and delete apply at
// once; the server reads the glossary for every line, so the next line uses it.
(() => {
  const el = (id) => document.getElementById(id);
  const esc = Yokoten.esc;
  const ui = { terms: [], profile: null };

  function say(text, ok) {
    el('gl-msg').textContent = text;
    el('gl-msg').className = `gl-note ${ok ? 'ok' : 'err'}`;
    el('gl-msg').hidden = !text;
  }

  // fetch + JSON; resolves to { ok, body, error } and never throws.
  async function send(method, path, body) {
    const res = await fetch(path, {
      method,
      headers: body ? { 'content-type': 'application/json' } : {},
      body: body ? JSON.stringify(body) : undefined,
    }).catch(() => null);
    const json = await res?.json().catch(() => null);
    return { ok: !!res?.ok, body: json, error: json?.error ?? 'ต่อ Yokoten ไม่ได้ ลองอีกครั้ง' };
  }

  async function load() {
    const res = await send('GET', '/api/glossary');
    if (res.ok) ui.terms = res.body;
    else say(res.error, false);
    render();
  }

  const DELETE_ICON = '<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 6h18"/><path d="M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6"/><path d="M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2"/></svg>';

  const row = (t, overridden) => `
    <li class="gl-row${overridden ? ' overridden' : ''}" data-id="${esc(t.id)}">
      <input class="input" data-field="source" value="${esc(t.source)}" maxlength="100" autocomplete="off" spellcheck="false" aria-label="คำต้นฉบับ">
      <input class="input" data-field="target" value="${esc(t.target)}" maxlength="100" autocomplete="off" aria-label="คำแปลของ ${esc(t.source)}">
      <button type="button" class="gl-del" aria-label="ลบ ${esc(t.source)}">${DELETE_ICON}</button>
      ${overridden ? '<span class="gl-note">เกมนี้ใช้คำของเกมแทน</span>' : ''}
    </li>`;

  const list = (items, empty) => items.join('') || `<li class="gl-empty">${empty}</li>`;

  function render() {
    const game = ui.profile ? ui.terms.filter((t) => t.profile_id === ui.profile.id) : [];
    const shared = ui.terms.filter((t) => t.profile_id === '');
    const own = new Set(game.map((t) => t.source.toLowerCase()));
    el('gl-game-title').textContent = ui.profile ? `ของเกม ${ui.profile.name}` : 'ของเกมนี้';
    el('gl-layer-game').textContent = ui.profile ? `เกมนี้ (${ui.profile.name})` : 'เกมนี้';
    el('gl-layer-game').disabled = !ui.profile;
    if (!ui.profile) el('gl-layer').value = 'shared';
    el('gl-game').innerHTML = list(game.map((t) => row(t, false)), 'ยังไม่มีคำของเกมนี้');
    el('gl-shared').innerHTML = list(
      shared.map((t) => row(t, own.has(t.source.toLowerCase()))),
      'ยังไม่มีคำกลาง',
    );
  }

  el('gl-add').onsubmit = async (e) => {
    e.preventDefault();
    say('');
    const game = el('gl-layer').value === 'game' && ui.profile;
    const res = await send('POST', '/api/glossary', {
      source: el('gl-source').value,
      target: el('gl-target').value,
      profile_id: game ? ui.profile.id : '',
    });
    if (!res.ok) return say(res.error, false);
    el('gl-source').value = '';
    el('gl-target').value = '';
    el('gl-source').focus();
    say(`เพิ่ม “${res.body.source}” แล้ว ใช้ตั้งแต่บรรทัดถัดไป`, true);
    await load();
  };

  // Edit on change; a refused edit puts the saved words back.
  const onChange = async (e) => {
    const li = e.target.closest('.gl-row');
    if (!li) return;
    say('');
    const value = (f) => li.querySelector(`[data-field="${f}"]`).value;
    const res = await send('PUT', `/api/glossary/${encodeURIComponent(li.dataset.id)}`, {
      source: value('source'),
      target: value('target'),
    });
    if (!res.ok) say(res.error, false);
    await load();
  };

  const onClick = async (e) => {
    const btn = e.target.closest('.gl-del');
    if (!btn) return;
    say('');
    const li = btn.closest('.gl-row');
    const res = await send('DELETE', `/api/glossary/${encodeURIComponent(li.dataset.id)}`);
    if (!res.ok) say(res.error, false);
    await load();
  };

  for (const id of ['gl-game', 'gl-shared']) {
    el(id).addEventListener('change', onChange);
    el(id).addEventListener('click', onClick);
  }

  let seen = '';
  Yokoten.follow({
    state: (s) => {
      ui.profile = s.profile ?? null;
      const key = `${s.profile?.id}|${s.profile?.name}`;
      if (key !== seen) { seen = key; load(); }
    },
  });
})();
