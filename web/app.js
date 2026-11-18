// Shared by /reader, /overlay, /setup: escape text, follow /events, switch translator.
window.Yokoten = (() => {
  const STATUS_LABEL = { idle: 'ว่าง', translating: 'กำลังแปล', error: 'มีปัญหา' };
  const esc = (s) => String(s).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);

  // Calls handlers[eventName](data) for each SSE event and onConnection(bool)
  // when the link to the server comes or goes (FR-WEB-07). EventSource retries
  // by itself; if the browser gives up (CLOSED), start a fresh one.
  function follow(handlers, onConnection = () => {}) {
    let es;
    const open = () => {
      es = new EventSource('/events');
      es.onopen = () => onConnection(true);
      es.onerror = () => {
        onConnection(false);
        if (es.readyState === EventSource.CLOSED) setTimeout(open, 3000);
      };
      Object.entries(handlers).forEach(([name, fn]) => {
        es.addEventListener(name, (e) => fn(JSON.parse(e.data)));
      });
    };
    open();
  }

  // Engines the user can pick (FR-TRN-02), in the order shown. The server
  // holds the one in use; every page follows it through the `state` event.
  const TRANSLATORS = [
    { id: 'apple', label: 'Apple Translation' },
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
