// /setup checklist (FR-ONB-06, #38, #42): GET /api/checklist only reads;
// each item links to the section that fixes it.
(() => {
  const el = (id) => document.getElementById(id);
  const ICON = {
    ok: '<path d="M20 6 9 17l-5-5"/>',
    fail: '<path d="M12 9v4"/><path d="M12 17h.01"/>',
    skip: '<path d="M5 12h14"/>',
  };
  const STATE_LABEL = { ok: 'พร้อม', fail: 'ต้องแก้', skip: 'ยังไม่ตรวจ' };
  const icon = (state) => `<span class="check-icon ${state}" aria-hidden="true"><svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">${ICON[state]}</svg></span>`;

  const row = (i) => {
    const esc = Yokoten.esc;
    const fix = i.state === 'fail' && i.fix ? `<p>${esc(i.fix)}</p>` : '';
    const go = i.state === 'fail'
      ? `<a class="btn check-go" href="#${esc(i.anchor)}">แก้</a>`
      : '';
    return `<li class="check">${icon(i.state)}
      <div class="check-body"><span><strong style="font-weight:500">${esc(i.label)}</strong> <span class="label">${STATE_LABEL[i.state]}</span></span>
        <p>${esc(i.detail)}</p>${fix}</div>${go}</li>`;
  };

  let busy = false;
  async function load() {
    if (busy) return;
    busy = true;
    el('checklist-refresh').disabled = true;
    try {
      const res = await fetch('/api/checklist');
      if (!res.ok) throw new Error(String(res.status));
      const list = await res.json();
      el('checks').innerHTML = list.items.map(row).join('');
      const failed = list.items.filter((i) => i.state === 'fail').length;
      el('checklist-summary').textContent = list.ready ? 'พร้อมเริ่มแปล' : `ต้องแก้ ${failed} รายการ`;
      el('checklist-err').hidden = true;
    } catch {
      el('checklist-err').textContent = 'ตรวจไม่ได้ ต่อ Yokoten ไม่ได้ ลองอีกครั้ง';
      el('checklist-err').hidden = false;
    } finally {
      busy = false;
      el('checklist-refresh').disabled = false;
    }
  }

  el('checklist-refresh').onclick = load;
  // Things fixed outside this page (OBS, System Settings) show when coming back.
  window.addEventListener('focus', load);
  // OBS setup, a new box or a new profile changes the answer.
  document.addEventListener('yokoten:changed', load);
  // The OBS and box sections write a result line when "ตั้ง OBS ให้" or
  // "ใช้กรอบนี้" finishes; neither sends a `state` event.
  for (const id of ['obs-result', 'region-result']) {
    const out = el(id);
    if (out) new MutationObserver(load).observe(out, { childList: true, characterData: true, subtree: true });
  }

  let seen = '';
  Yokoten.follow({
    state: (s) => {
      const key = [s.profile?.id, s.profile?.name, s.status, s.translator, s.error?.code].join('|');
      if (key !== seen) { seen = key; load(); }
    },
  });
})();
