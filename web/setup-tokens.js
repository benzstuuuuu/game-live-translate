// Gemini tokens of this session on /setup (FR-TRN-10): counts from
// AppState.tokens, updated with each `state` event. No cost is shown.
(() => {
  const el = document.getElementById('tr-tokens');
  const fmt = (n) => Number(n ?? 0).toLocaleString('th-TH');
  window.Yokoten.follow({
    state: (s) => {
      el.textContent = `token ของ session นี้: input ${fmt(s.tokens?.input)} · output ${fmt(s.tokens?.output)}`;
    },
  });
})();
