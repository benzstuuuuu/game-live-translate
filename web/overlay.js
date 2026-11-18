// /overlay (FR-STR-01 to 03): the latest Thai line in the bar. It shows on
// `line`, follows `line-updated` of the same line, hides on `subtitle-gone`,
// and moves at once when /setup sends `bar`.
(() => {
  const stage = document.getElementById('stage');
  const bar = document.getElementById('bar');
  const text = document.getElementById('text');

  const fit = () => {
    const scale = Math.min(innerWidth / 1920, innerHeight / 1080);
    stage.style.transform = `translate(-50%, -50%) scale(${scale})`;
  };
  addEventListener('resize', fit);
  fit();

  // At most two lines (FR-STR-01): a longer line is cut at a whole Thai
  // character cluster and ends in "…". Not clipped with CSS, because tone
  // marks of a hidden third line would still show under the second.
  // offsetHeight counts line boxes (88 px each); Thai marks that poke out of
  // a line would make scrollHeight look like an extra line.
  const TWO_LINES = 2 * 88;
  const clusters = (s) => [...new Intl.Segmenter('th', { granularity: 'grapheme' }).segment(s)].map((g) => g.segment);
  let full = '';
  function setText(s) {
    full = s;
    text.textContent = s;
    if (text.offsetHeight <= TWO_LINES) return;
    const parts = clusters(s);
    let lo = 0;
    let hi = parts.length;
    while (lo < hi) {
      const mid = Math.ceil((lo + hi) / 2);
      text.textContent = `${parts.slice(0, mid).join('').trimEnd()}…`;
      if (text.offsetHeight <= TWO_LINES) lo = mid;
      else hi = mid - 1;
    }
    text.textContent = `${parts.slice(0, lo).join('').trimEnd()}…`;
  }
  // Widths change when the Thai font arrives or the bar is resized.
  document.fonts.addEventListener('loadingdone', () => setText(full));

  let lineId = null;
  let profileId; // undefined until the first `state`, so that one loads the bar
  const show = (line) => {
    lineId = line.id;
    setText(line.target_text);
    bar.classList.remove('hidden');
  };
  const hide = () => bar.classList.add('hidden');

  // Fractions of 1920x1080 (bar_x, bar_y, bar_w of the profile).
  const place = (b) => {
    Object.assign(bar.style, { left: `${b.x * 100}%`, top: `${b.y * 100}%`, width: `${b.w * 100}%` });
    setText(full);
  };
  async function loadBar() {
    const res = await fetch('/api/bar').catch(() => null);
    const b = res?.ok ? await res.json().catch(() => null) : null;
    if (b) place(b);
  }

  Yokoten.follow(
    {
      state: (s) => {
        if (s.status !== 'translating') hide();
        const id = s.profile?.id ?? null;
        if (id !== profileId) {
          profileId = id;
          loadBar();
        }
      },
      line: show,
      'line-updated': (l) => {
        if (l.id === lineId) setText(l.target_text);
      },
      'subtitle-gone': hide,
      bar: place,
    },
    (up) => {
      // A dead server must not leave an old line on the stream; reload the
      // bar when it comes back.
      if (!up) {
        hide();
        profileId = undefined;
      }
    },
  );
})();
