// Shared by /reader, /overlay, /setup: escape text, follow /events, switch translator.
window.Yokoten = (() => {
  const STATUS_LABEL = { idle: 'ว่าง', translating: 'กำลังแปล', error: 'มีปัญหา' };
  const esc = (s) => String(s).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);

  // Calls handlers[eventName](data) for each SSE event and onConnection(bool)
  // when the link to the server comes or goes (FR-WEB-07). Every caller on a
  // page shares one EventSource; a late caller gets the last `state` at once.
  // EventSource retries by itself; if the browser gives up (CLOSED), start a
  // fresh one.
  const subs = [];
  const names = new Set();
  let es = null;
  let online = false;
  let lastState = null;

  function listen(name) {
    es.addEventListener(name, (e) => {
      const data = JSON.parse(e.data);
      if (name === 'state') lastState = data;
      subs.forEach((s) => s.handlers[name]?.(data));
    });
  }

  function open() {
    es = new EventSource('/events');
    es.onopen = () => { online = true; subs.forEach((s) => s.onConnection(true)); };
    es.onerror = () => {
      online = false;
      subs.forEach((s) => s.onConnection(false));
      if (es.readyState === EventSource.CLOSED) setTimeout(open, 3000);
    };
    names.forEach(listen);
  }

  function follow(handlers, onConnection = () => {}) {
    subs.push({ handlers, onConnection });
    if (!es) open();
    Object.keys(handlers).forEach((name) => {
      if (!names.has(name)) { names.add(name); listen(name); }
    });
    if (online) {
      onConnection(true);
      if (lastState) handlers.state?.(lastState);
    }
  }

  // Engines the user can pick, for live lines and for "แปลใหม่" alike
  // (FR-TRN-02, 16), in the order shown. The server holds both picks; every
  // page follows them through the `state` event.
  const TRANSLATORS = [
    { id: 'apple', label: 'Apple Translation' },
    { id: 'gemini', label: 'Gemini' },
    { id: 'fake', label: 'ทดสอบ (ไม่แปลจริง)' },
  ];

  // Switch the app-wide translator. Resolves to null, or a Thai error message.
  async function setTranslator(id) {
    const res = await fetch('/api/translator', {
      method: 'PUT',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ translator: id }),
    }).catch(() => null);
    if (res?.ok) return null;
    const body = await res?.json().catch(() => null);
    return body?.error ?? 'เปลี่ยนตัวแปลไม่สำเร็จ ลองอีกครั้ง';
  }

  // One segmented button per engine; `current` gets aria-pressed.
  const translatorButtons = (current, disabled) => TRANSLATORS.map((t) =>
    `<button type="button" class="seg-btn" data-translator="${esc(t.id)}" aria-pressed="${t.id === current}"${disabled ? ' disabled' : ''}>${esc(t.label)}</button>`).join('');

  return { STATUS_LABEL, TRANSLATORS, esc, follow, setTranslator, translatorButtons };
})();
