// /reader line sheet (FR-WEB-05, #45): copy a line, translate it again with
// another engine (FR-TRN-08; the new text comes back as `line-updated`), or
// add a term to the glossary of this game or of all games (FR-GLS-02). The
// page picks the line and calls open() then render(line, online).
window.ReaderActions = (() => {
  const el = (id) => document.getElementById(id);
  const label = (id) => Yokoten.TRANSLATORS.find((t) => t.id === id)?.label ?? id;
  const ui = { line: null, online: false, busy: false };

  // Engine name, shown only on lines the backup translated.
  const engineTag = (line) => (line?.is_fallback ? `แปลโดย ${label(line.engine)} · ตัวสำรอง` : '');
  // "เสียง" on audio lines, only when the history mixes both sources
  // (FR-AUD-06); with one source it says nothing new.
  const sourceTag = (line, lines) => {
    const mixed = lines.some((l) => l.source === 'audio') && lines.some((l) => l.source !== 'audio');
    return mixed && line.source === 'audio' ? 'เสียง' : '';
  };

  function say(msg, err) {
    el('line-msg').textContent = msg ?? '';
    el('line-msg').hidden = !msg;
    el('line-err').textContent = err ?? '';
    el('line-err').hidden = !err;
  }

  // A newly picked line: empty form, no old messages.
  function open() {
    el('gls-source').value = '';
    el('gls-target').value = '';
    say();
  }

  // Redraws text and engine buttons only, so typing in the form survives a
  // `line-updated` for this line.
  function render(line, online) {
    ui.line = line;
    ui.online = online;
    const tag = engineTag(line);
    el('line-engine').hidden = !tag;
    el('line-engine').textContent = tag;
    el('line-th').textContent = line.target_text;
    el('line-en').textContent = line.source_text;
    const off = !online || ui.busy ? ' disabled' : '';
    el('line-retranslate').innerHTML = Yokoten.TRANSLATORS
      .filter((t) => t.id !== line.engine)
      .map((t) => `<button type="button" class="seg-btn" data-retranslate="${Yokoten.esc(t.id)}"${off}>${Yokoten.esc(t.label)}</button>`)
      .join('');
    el('gls-add').disabled = !online;
  }

  async function post(path, body) {
    const res = await fetch(path, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(body),
    }).catch(() => null);
    if (res?.ok) return null;
    const data = await res?.json().catch(() => null);
    return data?.error ?? 'สั่งงานไม่สำเร็จ ลองอีกครั้ง';
  }

  // The Clipboard API needs https or localhost; the phone opens plain http
  // on the LAN, so fall back to a selected textarea.
  async function copy(text) {
    try {
      await navigator.clipboard.writeText(text);
      return true;
    } catch { /* not a secure context */ }
    const ta = document.createElement('textarea');
    ta.value = text;
    ta.setAttribute('readonly', '');
    ta.style.position = 'fixed';
    ta.style.opacity = '0';
    document.body.appendChild(ta);
    ta.select();
    ta.setSelectionRange(0, text.length);
    let ok = false;
    try { ok = document.execCommand('copy'); } catch { /* unsupported */ }
    ta.remove();
    return ok;
  }

  el('line-copy').addEventListener('click', async () => {
    if (!ui.line) return;
    const ok = await copy(ui.line.target_text);
    say(ok ? 'คัดลอกแล้ว' : null, ok ? null : 'คัดลอกไม่ได้ กดค้างที่ข้อความเพื่อคัดลอกเอง');
  });

  el('line-retranslate').addEventListener('click', async (e) => {
    const t = e.target.closest('[data-retranslate]');
    if (!t || !ui.line || ui.busy) return;
    const engine = t.dataset.retranslate;
    ui.busy = true;
    say(`กำลังแปลใหม่ด้วย ${label(engine)}…`);
    render(ui.line, ui.online);
    const err = await post(`/api/lines/${encodeURIComponent(ui.line.id)}/retranslate`, { translator: engine });
    ui.busy = false;
    say(err ? null : `แปลใหม่ด้วย ${label(engine)} แล้ว`, err);
    render(ui.line, ui.online);
  });

  el('gls').addEventListener('submit', async (e) => {
    e.preventDefault();
    if (!ui.line) return;
    const source = el('gls-source').value;
    const scope = document.querySelector('input[name="gls-scope"]:checked')?.value;
    el('gls-add').disabled = true;
    const err = await post('/api/glossary', {
      source,
      target: el('gls-target').value,
      profile_id: scope === 'all' ? '' : ui.line.profile_id,
    });
    el('gls-add').disabled = !ui.online;
    if (err) { say(null, err); return; }
    el('gls-source').value = '';
    el('gls-target').value = '';
    say(`เพิ่ม “${source.trim()}” แล้ว ใช้กับบรรทัดถัดไป`);
  });

  // The line blocks are role="button": Enter or Space opens them too.
  document.addEventListener('keydown', (e) => {
    if ((e.key === 'Enter' || e.key === ' ') && e.target.matches?.('[data-line]')) {
      e.preventDefault();
      e.target.click();
    }
  });

  return { engineTag, sourceTag, open, render };
})();
