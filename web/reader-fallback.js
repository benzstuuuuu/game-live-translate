// /reader fallback card (FR-TRN-05, #37): while the selected translator
// (Gemini or Claude) fails or is too slow, lines come from Apple. The server
// tries it again by itself every 30 seconds; "ลอง … อีกครั้ง" picks it again
// so the next line tries it at once. The card goes when a line from it works.
(() => {
  const el = (id) => document.getElementById(id);
  const ui = { state: null, online: false, asked: false };

  function render() {
    const s = ui.state;
    const fb = ui.online && s?.status === 'translating' && s.fallback?.active ? s.fallback : null;
    if (!fb) ui.asked = false;
    const name = Yokoten.TRANSLATORS.find((t) => t.id === s?.translator)?.label ?? 'Gemini';
    el('fallback').hidden = !fb;
    el('fallback-reason').textContent = fb?.reason ?? '';
    el('fallback-note').textContent = ui.asked ? `จะลอง ${name} กับบรรทัดถัดไป` : `ระบบลอง ${name} เองทุก 30 วินาที`;
    el('fallback-retry-label').textContent = `ลอง ${name} อีกครั้ง`;
    el('fallback-retry').disabled = !ui.online || ui.asked;
  }

  el('fallback-retry').addEventListener('click', async () => {
    el('fallback-err').hidden = true;
    const err = await Yokoten.setTranslator(ui.state?.translator ?? 'gemini');
    ui.asked = !err;
    el('fallback-err').textContent = err ?? '';
    el('fallback-err').hidden = !err;
    render();
  });

  Yokoten.follow(
    {
      state: (s) => {
        // A new reason means the retry ran and failed; allow another.
        if (s.fallback?.reason !== ui.state?.fallback?.reason) ui.asked = false;
        ui.state = s;
        ui.online = true;
        render();
      },
      // The next line came and the card is still up: that try failed too.
      line: () => { ui.asked = false; render(); },
    },
    (up) => { ui.online = up; render(); },
  );
})();
