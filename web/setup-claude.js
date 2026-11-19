// Claude key on /setup (FR-TRN-14, FR-SET-07): save to the Keychain and test,
// like the Gemini key in setup-translate.js, and pick the model and effort.
// The key never comes back from the server; GET only says whether one is saved.
(() => {
  const $ = (id) => document.getElementById(id);
  const PATH = '/api/secret/anthropic-key';

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

  async function load() {
    const [ok, body] = await send('GET', PATH);
    if (!ok || !body) note('cl-key-state', 'โหลดสถานะ key ไม่ได้ ลองเปิดหน้านี้ใหม่', 'err');
    else if (body.key_saved) note('cl-key-state', `บันทึก key ไว้ใน Keychain แล้ว ใช้ ${body.model} ช่องนี้ว่างไว้ได้ถ้าไม่เปลี่ยน key`, 'ok');
    else note('cl-key-state', 'ยังไม่มี key', '');
  }

  $('cl-key-save').onclick = async () => {
    $('cl-key-save').disabled = true;
    const [ok, body] = await send('PUT', PATH, { key: $('cl-key').value });
    $('cl-key-save').disabled = false;
    if (ok) {
      $('cl-key').value = '';
      note('cl-key-result', 'บันทึก key แล้ว', 'ok');
      load();
    } else {
      note('cl-key-result', body?.error ?? 'บันทึก key ไม่สำเร็จ', 'err');
    }
  };

  // Tests the typed key, or the saved one when the field is empty.
  $('cl-key-test').onclick = async () => {
    const key = $('cl-key').value.trim();
    $('cl-key-test').disabled = true;
    note('cl-key-result', 'กำลังทดสอบ…', '');
    const [ok, body] = await send('POST', `${PATH}/test`, key ? { key } : {});
    $('cl-key-test').disabled = false;
    if (ok && body?.ok) {
      const sec = (body.ms / 1000).toFixed(1);
      note('cl-key-result', `ใช้ได้ · ${body.model} ตอบใน ${sec} วินาที · ตัวอย่าง: ${body.message}`, 'ok');
    } else {
      note('cl-key-result', `ใช้ไม่ได้: ${body?.message ?? body?.error ?? 'ต่อ Yokoten ไม่ได้'}`, 'err');
    }
  };

  // Model and effort (#97): saved on change, used from the next line.
  const PICK = '/api/claude';

  function showPick(body) {
    for (const [id, choices, picked] of [
      ['cl-model', body.models, body.model],
      ['cl-effort', body.efforts, body.effort],
    ]) {
      $(id).replaceChildren(...choices.map((c) => new Option(c, c, false, c === picked)));
      $(id).disabled = false;
    }
  }

  async function loadPick() {
    const [ok, body] = await send('GET', PICK);
    if (ok && body) showPick(body);
    else note('cl-pick-result', 'โหลด model ของ Claude ไม่ได้ ลองเปิดหน้านี้ใหม่', 'err');
  }

  async function changePick(field, value) {
    $('cl-model').disabled = true;
    $('cl-effort').disabled = true;
    const [ok, body] = await send('PUT', PICK, { [field]: value });
    if (ok && body) {
      showPick(body);
      note('cl-pick-result', `บรรทัดถัดไปใช้ ${body.model} effort ${body.effort}`, 'ok');
      load();
    } else {
      note('cl-pick-result', body?.error ?? 'เปลี่ยน model ไม่สำเร็จ', 'err');
      loadPick();
    }
  }

  $('cl-model').addEventListener('change', (e) => changePick('model', e.target.value));
  $('cl-effort').addEventListener('change', (e) => changePick('effort', e.target.value));

  load();
  loadPick();
})();
