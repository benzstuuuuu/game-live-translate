// Shared by /reader, /overlay, /setup: escape text and follow /events.
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

  return { STATUS_LABEL, esc, follow };
})();
