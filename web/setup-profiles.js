// /setup game section (#38): pick the profile in use, create one, rename it,
// clear its history or delete it. Every change applies at once; destructive
// ones ask first. Other sections listen for `yokoten:changed`.
(() => {
  const el = (id) => document.getElementById(id);
  const ui = { profiles: [], current: null, translating: false };

  const changed = () => document.dispatchEvent(new CustomEvent('yokoten:changed'));

  function say(text, ok) {
    el('game-msg').textContent = text;
    el('game-msg').className = ok ? 'ok' : 'err';
    el('game-msg').hidden = !text;
  }

  // fetch + JSON; resolves to { ok, body } and never throws.
  async function send(method, path, body) {
    const res = await fetch(path, {
      method,
      headers: body ? { 'content-type': 'application/json' } : {},
      body: body ? JSON.stringify(body) : undefined,
    }).catch(() => null);
    const json = await res?.json().catch(() => null);
    const error = typeof json?.error === 'string' ? json.error : json?.error?.message;
    return { ok: !!res?.ok, body: json, error: error ?? 'ต่อ Yokoten ไม่ได้ ลองอีกครั้ง' };
  }

  async function load() {
    const res = await send('GET', '/api/profiles');
    if (res.ok) ui.profiles = res.body;
    render();
  }

  function render() {
    const esc = Yokoten.esc;
    const cur = ui.profiles.find((p) => p.id === ui.current) ?? null;
    el('profile-pick').innerHTML = ui.profiles.map((p) =>
      `<option value="${esc(p.id)}"${p.id === ui.current ? ' selected' : ''}>${esc(p.name)}</option>`).join('');
    if (document.activeElement !== el('profile-name')) el('profile-name').value = cur?.name ?? '';
    el('profile-region').textContent = !cur ? '' : cur.has_region ? 'มีแล้ว' : 'ยังไม่มี ลากกรอบบนภาพจากการ์ด';
    el('profile-lines').textContent = cur ? `${cur.line_count.toLocaleString('th-TH')} บรรทัด` : '';
    for (const id of ['profile-name', 'profile-clear', 'profile-delete']) el(id).disabled = !cur;
  }

  // Ask before anything that cannot be undone. Resolves to true on OK.
  function confirmDanger(title, text, ok) {
    const d = el('profile-confirm');
    el('profile-confirm-title').textContent = title;
    el('profile-confirm-text').textContent = text;
    el('profile-confirm-ok').textContent = ok;
    d.returnValue = '';
    d.showModal();
    return new Promise((resolve) => {
      d.addEventListener('close', () => resolve(d.returnValue === 'ok'), { once: true });
    });
  }

  el('profile-pick').onchange = async (e) => {
    say('');
    const res = await send('PUT', '/api/profile', { id: e.target.value });
    if (!res.ok) say(res.error, false);
    else changed();
    // The `state` event redraws the choice.
  };

  el('profile-new').onclick = () => {
    el('profile-create').hidden = false;
    el('profile-create-name').focus();
  };
  el('profile-create-cancel').onclick = () => {
    el('profile-create').hidden = true;
    el('profile-create-name').value = '';
  };
  el('profile-create').onsubmit = async (e) => {
    e.preventDefault();
    say('');
    const res = await send('POST', '/api/profiles', { name: el('profile-create-name').value });
    if (!res.ok) return say(res.error, false);
    el('profile-create').hidden = true;
    el('profile-create-name').value = '';
    // Switch to it unless that would cut a running session short.
    if (!ui.translating) await send('PUT', '/api/profile', { id: res.body.id });
    await load();
    say(`สร้าง “${res.body.name}” แล้ว${ui.translating ? ' เลือกได้เมื่อหยุดแปล' : ' ต่อไปลากกรอบ subtitle'}`, true);
    changed();
  };

  // Rename on change (Enter or leaving the box); no save button.
  el('profile-name').onchange = async (e) => {
    say('');
    const id = ui.current;
    const res = await send('PUT', `/api/profiles/${encodeURIComponent(id)}`, { name: e.target.value });
    if (!res.ok) {
      say(res.error, false);
      e.target.value = ui.profiles.find((p) => p.id === id)?.name ?? '';
      return;
    }
    await load();
  };

  el('profile-clear').onclick = async () => {
    const cur = ui.profiles.find((p) => p.id === ui.current);
    if (!cur) return;
    const ok = await confirmDanger(
      `ล้างประวัติของ “${cur.name}”?`,
      `ลบคำแปลที่เก็บไว้ ${cur.line_count.toLocaleString('th-TH')} บรรทัด กู้คืนไม่ได้ โปรไฟล์และกรอบยังอยู่`,
      'ล้างประวัติ',
    );
    if (!ok) return;
    const res = await send('DELETE', `/api/profiles/${encodeURIComponent(cur.id)}/lines`);
    if (!res.ok) return say(res.error, false);
    say(`ล้างแล้ว ${res.body.deleted.toLocaleString('th-TH')} บรรทัด`, true);
    await load();
  };

  el('profile-delete').onclick = async () => {
    const cur = ui.profiles.find((p) => p.id === ui.current);
    if (!cur) return;
    const ok = await confirmDanger(
      `ลบโปรไฟล์ “${cur.name}”?`,
      `ลบกรอบ subtitle และประวัติ ${cur.line_count.toLocaleString('th-TH')} บรรทัดของเกมนี้ กู้คืนไม่ได้${ui.translating ? ' และหยุดแปลที่กำลังทำอยู่' : ''}`,
      'ลบโปรไฟล์',
    );
    if (!ok) return;
    const res = await send('DELETE', `/api/profiles/${encodeURIComponent(cur.id)}`);
    if (!res.ok) return say(res.error, false);
    say(`ลบ “${cur.name}” แล้ว`, true);
    await load();
    changed();
  };

  // A box saved in the subtitle-box section changes "กรอบ subtitle" here.
  const regionResult = el('region-result');
  if (regionResult) new MutationObserver(load).observe(regionResult, { childList: true, characterData: true, subtree: true });

  let seen = '';
  Yokoten.follow({
    state: (s) => {
      ui.current = s.profile?.id ?? null;
      ui.translating = s.status === 'translating';
      // Reload counts when the profile or session changes (lines come per session).
      const key = `${s.profile?.id}|${s.profile?.name}|${s.session_id}|${s.status}`;
      if (key !== seen) { seen = key; load(); } else render();
    },
  });
})();
