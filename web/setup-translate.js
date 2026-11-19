// Translation section of /setup (FR-SET-03, 07): Gemini key, models with their
// thinking level, the translator for "แปลใหม่", context lines, tone and extra
// prompt. Every change is saved at once with PUT /api/settings (or
// /api/retranslator); the key never comes back from the server.
(() => {
  const $ = (id) => document.getElementById(id);
  const { esc } = window.Yokoten;
  const TRASH = '<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 6h18"/><path d="M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6"/><path d="M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2"/><line x1="10" x2="10" y1="11" y2="17"/><line x1="14" x2="14" y1="11" y2="17"/></svg>';
  const LEVEL_LABEL = { default: 'ค่าเริ่มต้นของ model', minimal: 'minimal', low: 'low', medium: 'medium', high: 'high', xhigh: 'xhigh' };

  let current = null;

  // Resolves to [ok, body]; body is the JSON reply or null.
  async function send(method, path, payload) {
    const res = await fetch(path, {
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

  function render(s) {
    current = s;
    note('tr-key-state', s.gemini.key_saved ? 'บันทึก key ไว้ใน Keychain แล้ว ช่องนี้ว่างไว้ได้ถ้าไม่เปลี่ยน key' : 'ยังไม่มี key', s.gemini.key_saved ? 'ok' : '');
    const single = s.gemini.models.length === 1;
    $('tr-models').innerHTML = s.gemini.models.map((m) => `
      <tr data-id="${esc(m.id)}">
        <td><input type="radio" name="tr-live" value="${esc(m.id)}" aria-label="ใช้ ${esc(m.id)} แปลสด"${m.id === s.gemini.live_model ? ' checked' : ''}></td>
        <td><code>${esc(m.id)}</code></td>
        <td><select class="tr-select" aria-label="thinking ของ ${esc(m.id)}">${m.levels.map((l) =>
          `<option value="${esc(l)}"${l === m.thinking ? ' selected' : ''}>${esc(LEVEL_LABEL[l] ?? l)}</option>`).join('')}</select></td>
        <td><button type="button" class="tr-icon-btn" data-remove aria-label="ลบ ${esc(m.id)}"${single ? ' disabled' : ''}>${TRASH}</button></td>
      </tr>`).join('');
    // Empty = the live model (FR-TRN-13).
    $('tr-quality').innerHTML = [['', 'ตัวเดียวกับแปลสด'], ...s.gemini.models.map((m) => [m.id, m.id])].map(([v, label]) =>
      `<option value="${esc(v)}"${v === s.gemini.quality_model ? ' selected' : ''}>${esc(label)}</option>`).join('');
    $('tr-context').value = s.context_lines;
    document.querySelectorAll('#tr-tone [data-tone]').forEach((b) => b.setAttribute('aria-pressed', String(b.dataset.tone === s.tone)));
    if (document.activeElement !== $('tr-prompt')) $('tr-prompt').value = s.extra_prompt;
  }

  async function load() {
    const [ok, body] = await send('GET', '/api/settings');
    if (ok && body) render(body);
    else note('tr-status', 'โหลดค่าตั้งไม่ได้ ลองเปิดหน้านี้ใหม่', 'err');
  }

  // Saves the keys in `patch`; on refusal shows why and puts the old values back.
  async function save(patch) {
    const [ok, body] = await send('PUT', '/api/settings', patch);
    if (ok && body) {
      render(body);
      note('tr-status', 'บันทึกแล้ว', 'ok');
    } else {
      if (current) render(current);
      note('tr-status', body?.error ?? 'บันทึกไม่สำเร็จ ลองอีกครั้ง', 'err');
    }
  }

  const models = () => current.gemini.models.map(({ id, thinking }) => ({ id, thinking }));

  $('tr-key-save').onclick = async () => {
    const key = $('tr-key').value;
    $('tr-key-save').disabled = true;
    const [ok, body] = await send('PUT', '/api/secret/gemini-key', { key });
    $('tr-key-save').disabled = false;
    if (ok) {
      $('tr-key').value = '';
      note('tr-key-result', 'บันทึก key แล้ว', 'ok');
      load();
    } else {
      note('tr-key-result', body?.error ?? 'บันทึก key ไม่สำเร็จ', 'err');
    }
  };

  // Tests the typed key, or the saved one when the field is empty.
  $('tr-key-test').onclick = async () => {
    const key = $('tr-key').value.trim();
    $('tr-key-test').disabled = true;
    note('tr-key-result', 'กำลังทดสอบ…', '');
    const [ok, body] = await send('POST', '/api/secret/gemini-key/test', key ? { key } : {});
    $('tr-key-test').disabled = false;
    if (ok && body?.ok) {
      const sec = (body.ms / 1000).toFixed(1);
      note('tr-key-result', `ใช้ได้ · ${body.model} ตอบใน ${sec} วินาที · ตัวอย่าง: ${body.message}`, 'ok');
    } else {
      note('tr-key-result', `ใช้ไม่ได้: ${body?.message ?? body?.error ?? 'ต่อ Yokoten ไม่ได้'}`, 'err');
    }
  };

  $('tr-models').addEventListener('change', (e) => {
    const row = e.target.closest('tr[data-id]');
    if (!row) return;
    if (e.target.name === 'tr-live') {
      save({ gemini: { live_model: row.dataset.id } });
    } else if (e.target.tagName === 'SELECT') {
      save({ gemini: { models: models().map((m) => (m.id === row.dataset.id ? { ...m, thinking: e.target.value } : m)) } });
    }
  });

  $('tr-models').addEventListener('click', (e) => {
    const row = e.target.closest('[data-remove]')?.closest('tr[data-id]');
    if (row) save({ gemini: { models: models().filter((m) => m.id !== row.dataset.id) } });
  });

  // An empty thinking level lets the server pick the model's first one.
  $('tr-model-add').onclick = async () => {
    const id = $('tr-model-new').value.trim();
    if (!id || !current) return;
    await save({ gemini: { models: [...models(), { id, thinking: '' }] } });
    if (!$('tr-status').classList.contains('err')) $('tr-model-new').value = '';
  };

  $('tr-quality').addEventListener('change', (e) => save({ gemini: { quality_model: e.target.value } }));

  // FR-TRN-16: the translator for "แปลใหม่" on /reader, from AppState; empty =
  // the live one. Gemini's model for it (FR-TRN-13) shows only when that
  // translator is Gemini. Redrawn only when a pick changes, so an open
  // select is not reset by other `state` events.
  let picks = null;
  function renderRetranslator(s) {
    const key = `${s.translator}|${s.retranslator ?? ''}`;
    if (key === picks) return;
    picks = key;
    const live = window.Yokoten.TRANSLATORS.find((t) => t.id === s.translator)?.label ?? s.translator;
    $('tr-retranslator').innerHTML = [['', `ตัวเดียวกับแปลสด (${live})`], ...window.Yokoten.TRANSLATORS.map((t) => [t.id, t.label])]
      .map(([v, label]) => `<option value="${esc(v)}"${v === (s.retranslator ?? '') ? ' selected' : ''}>${esc(label)}</option>`).join('');
    $('tr-quality-row').hidden = (s.retranslator ?? s.translator) !== 'gemini';
  }

  let lastState = null;
  window.Yokoten.follow({ state: (s) => { lastState = s; renderRetranslator(s); } });

  $('tr-retranslator').addEventListener('change', async (e) => {
    const [ok, body] = await send('PUT', '/api/retranslator', { translator: e.target.value || null });
    if (ok && body) {
      renderRetranslator(body);
      note('tr-status', 'บันทึกแล้ว', 'ok');
    } else {
      picks = null;
      if (lastState) renderRetranslator(lastState);
      note('tr-status', body?.error ?? 'บันทึกไม่สำเร็จ ลองอีกครั้ง', 'err');
    }
  });

  $('tr-context').addEventListener('change', (e) => {
    const n = Number(e.target.value);
    if (e.target.value !== '' && Number.isInteger(n)) save({ context_lines: n });
    else if (current) render(current);
  });

  $('tr-tone').addEventListener('click', (e) => {
    const b = e.target.closest('[data-tone]');
    if (b && b.getAttribute('aria-pressed') !== 'true') save({ tone: b.dataset.tone });
  });

  $('tr-prompt').addEventListener('change', (e) => save({ extra_prompt: e.target.value }));

  load();
})();
