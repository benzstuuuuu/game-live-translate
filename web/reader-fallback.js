// /reader fallback card (FR-TRN-05, #37): while Gemini fails or is too slow,
// lines come from Apple. The server tries Gemini again by itself every 30
// seconds; "ลอง Gemini อีกครั้ง" picks Gemini again so the next line tries it
// at once. The card goes when a line from Gemini works.
(() => {
  const el = (id) => document.getElementById(id);
  const AUTO = 'ระบบลอง Gemini เองทุก 30 วินาที';
  const ui = { state: null, online: false, asked: false };

  function render() {
    const s = ui.state;
    const fb = ui.online && s?.status === 'translating' && s.fallback?.active ? s.fallback : null;
    if (!fb) ui.asked = false;
    el('fallback').hidden = !fb;
    el('fallback-reason').textContent = fb?.reason ?? '';
    el('fallback-note').textContent = ui.asked ? 'จะลอง Gemini กับบรรทัดถัดไป' : AUTO;
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
