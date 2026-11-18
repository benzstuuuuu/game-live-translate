// /reader profile picker in the control sheet (FR-WEB-03, #38). Picking only;
// the header follows the `state` event. Switching while translating ends the
// session and starts a new one on the server.
(() => {
  const el = (id) => document.getElementById(id);
  const ui = { profiles: [], state: null, online: false };

  function render() {
    const esc = Yokoten.esc;
    const cur = ui.state?.profile?.id;
    el('profiles').innerHTML = ui.profiles.map((p) =>
      `<button type="button" class="seg-btn" data-profile="${esc(p.id)}" aria-pressed="${p.id === cur}"${ui.online ? '' : ' disabled'}>${esc(p.name)}</button>`).join('');
    el('profiles-hint').hidden = ui.state?.status !== 'translating' || ui.profiles.length < 2;
  }

  async function load() {
    try {
      const res = await fetch('/api/profiles');
      if (res.ok) ui.profiles = await res.json();
    } catch { /* offline; the next open retries */ }
    render();
  }

  el('profiles').addEventListener('click', async (e) => {
    const t = e.target.closest('[data-profile]');
    if (!t || t.getAttribute('aria-pressed') === 'true') return;
    el('profile-err').hidden = true;
    const res = await fetch('/api/profile', {
      method: 'PUT',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ id: t.dataset.profile }),
    }).catch(() => null);
    if (res?.ok) return;
    const body = await res?.json().catch(() => null);
    el('profile-err').textContent = body?.error ?? 'เปลี่ยนโปรไฟล์ไม่สำเร็จ ลองอีกครั้ง';
    el('profile-err').hidden = false;
    load();
  });

  // Fresh list each time the control sheet opens (profiles change on /setup).
  new MutationObserver(() => { if (!el('sheet-controls').hidden) load(); })
    .observe(el('sheet-controls'), { attributes: true, attributeFilter: ['hidden'] });

  Yokoten.follow(
    { state: (s) => { ui.state = s; ui.online = true; render(); } },
    (up) => { ui.online = up; render(); },
  );
  load();
})();
