// Codex on /setup (FR-TRN-15): whether the `codex` on this Mac can
// translate (installed, logged in) with the fix when it cannot, and the
// model it runs. No key: it uses the user's own login.
(() => {
  const $ = (id) => document.getElementById(id);
  const PATH = '/api/codex-cli';

  // Resolves to [ok, body]; body is the JSON reply or null.
  async function send(method, payload) {
    const res = await fetch(PATH, {
      method,
      headers: payload === undefined ? {} : { 'content-type': 'application/json' },
      body: payload === undefined ? undefined : JSON.stringify(payload),
    }).catch(() => null);
    const body = await res?.json().catch(() => null);
    return [Boolean(res?.ok), body];
  }

  function note(id, text, kind) {
    const el = $(id);
    el.textContent = text;
    el.className = `tr-note${kind ? ` ${kind}` : ''}`;
    el.hidden = !text;
  }

  function show(body) {
    if (body.ready) note('cx-state', `พร้อมใช้ · model ${body.model} · effort ${body.effort}`, 'ok');
    else note('cx-state', `${body.message} · ${body.fix}`, 'err');
    const select = $('cx-model');
    select.replaceChildren(...body.models.map((m) => new Option(m, m, false, m === body.model)));
    select.disabled = false;
  }

  async function load() {
    const [ok, body] = await send('GET');
    if (ok && body) show(body);
    else note('cx-state', 'โหลดสถานะ Codex ไม่ได้ ลองเปิดหน้านี้ใหม่', 'err');
  }

  $('cx-model').addEventListener('change', async (e) => {
    e.target.disabled = true;
    const [ok, body] = await send('PUT', { model: e.target.value });
    e.target.disabled = false;
    if (ok && body) {
      show(body);
      note('cx-result', `บรรทัดถัดไปใช้ ${body.model}`, 'ok');
    } else {
      note('cx-result', body?.error ?? 'เปลี่ยน model ไม่สำเร็จ', 'err');
      load();
    }
  });

  load();
})();
