// PROTOTYPE (#25) — throwaway. Shared bits for the three prototype pages:
// fake lines, a fake live feed, the ?variant= / ?state= switcher bar.
// Nothing here talks to a server; everything lives in memory.
window.Proto = (() => {
  const LINES = [
    { en: 'Keep your lantern low. They follow the light.', th: 'หรี่ตะเกียงลงหน่อย พวกมันตามแสงมา' },
    { en: 'You sure about this, Mara?', th: 'แน่ใจนะ มาร่า' },
    { en: "No. But it's the only road left.", th: 'ไม่หรอก แต่เหลือทางนี้ทางเดียวแล้ว' },
    { en: "The bridge fell two winters ago. We'll have to swim across.", th: 'สะพานพังไปตั้งแต่สองหน้าหนาวก่อน เราต้องว่ายข้ามไป' },
    { en: 'Hold still. Something is moving under the ice.', th: 'อยู่นิ่ง ๆ มีอะไรขยับอยู่ใต้น้ำแข็ง' },
    { en: 'If the watchtower is still lit, the old keeper must be alive.', th: 'ถ้าหอสังเกตการณ์ยังมีไฟ คนเฝ้าคนเก่าก็คงยังไม่ตาย' },
    { en: 'We rest here tonight. Tomorrow we climb.', th: 'คืนนี้พักที่นี่ พรุ่งนี้ค่อยปีนขึ้นไป' },
    { en: 'Did you hear that? It sounded like someone singing.', th: 'ได้ยินไหม เหมือนมีคนร้องเพลง' },
    { en: "Stay behind me, and don't touch the stones.", th: 'อยู่ข้างหลังฉันไว้ แล้วอย่าแตะก้อนหิน' },
    { en: 'Whoever built this place wanted it forgotten.', th: 'ใครก็ตามที่สร้างที่นี่ ตั้งใจให้มันถูกลืม' },
  ];
  const PROFILES = ['Genshin Impact', 'Lantern Road', 'Death Stranding 2'];
  const STATES = ['translating', 'idle', 'error'];
  const STATE_LABEL = { translating: 'กำลังแปล', idle: 'ว่าง', error: 'มีปัญหา' };

  const params = new URLSearchParams(location.search);
  const pick = (key, keys) => (keys.includes(params.get(key)) ? params.get(key) : keys[0]);
  const go = (key, value) => {
    params.set(key, value);
    location.search = params.toString();
  };
  const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]);

  // Fake feed: a new line every `ms`, then "subtitle gone" `goneMs` later. Returns a stop fn.
  function feed(onLine, { ms = 4500, goneMs = 3200, onGone = () => {}, start = 0 } = {}) {
    // ?live=<SSE url> (spike/live-translate/tools/live_bridge.py): real Live Translate lines instead of fake ones.
    const live = new URLSearchParams(location.search).get('live');
    if (live) {
      let goneTimer;
      const es = new EventSource(live);
      es.addEventListener('line', (e) => { clearTimeout(goneTimer); onLine({ ...JSON.parse(e.data), at: new Date() }); });
      es.addEventListener('gone', () => { clearTimeout(goneTimer); goneTimer = setTimeout(onGone, goneMs); });
      return () => { es.close(); clearTimeout(goneTimer); };
    }
    let i = start;
    let goneTimer;
    const tick = () => {
      clearTimeout(goneTimer);
      onLine({ ...LINES[i % LINES.length], id: i, at: new Date() });
      goneTimer = setTimeout(onGone, goneMs);
      i += 1;
    };
    tick();
    const t = setInterval(tick, ms);
    return () => { clearInterval(t); clearTimeout(goneTimer); };
  }

  // Floating bar: ← variant →  plus extra toggles ([{label, key, values}]). Not part of the design.
  function switcher(variants, current, extras = []) {
    const idx = variants.findIndex((v) => v.key === current);
    const step = (d) => go('variant', variants[(idx + d + variants.length) % variants.length].key);
    const bar = document.createElement('div');
    bar.setAttribute('data-proto-bar', '');
    bar.style.cssText = 'position:fixed;left:50%;bottom:12px;transform:translateX(-50%);z-index:99999;display:flex;gap:6px;align-items:center;padding:6px;border-radius:9999px;background:#111;color:#fff;font:500 13px/1 system-ui,sans-serif;box-shadow:0 6px 24px rgba(0,0,0,.45);white-space:nowrap';
    const btn = (label, fn) => {
      const b = document.createElement('button');
      b.type = 'button';
      b.textContent = label;
      b.style.cssText = 'min-height:32px;padding:0 12px;border:0;border-radius:9999px;background:#333;color:#fff;font:inherit;cursor:pointer';
      b.onclick = fn;
      return b;
    };
    bar.append(btn('←', () => step(-1)));
    const label = document.createElement('span');
    label.style.padding = '0 6px';
    label.textContent = `${current} — ${variants[idx].name}`;
    bar.append(label, btn('→', () => step(1)));
    extras.forEach(({ label: l, key, values, names = {} }) => {
      const cur = pick(key, values);
      bar.append(btn(`${l}: ${names[cur] ?? cur}`, () => go(key, values[(values.indexOf(cur) + 1) % values.length])));
    });
    document.body.append(bar);
    addEventListener('keydown', (e) => {
      if (e.target.closest('input,textarea,select,[contenteditable]')) return;
      if (e.key === 'ArrowLeft') step(-1);
      if (e.key === 'ArrowRight') step(1);
    });
  }

  // Drag to move, drag the corner handle to resize. Values are fractions 0–1 of the container.
  function dragRect(rect, box, onChange) {
    rect.addEventListener('pointerdown', (e) => {
      const resize = e.target.hasAttribute('data-handle');
      const b = box.getBoundingClientRect();
      const r = rect.getBoundingClientRect();
      const sx = e.clientX, sy = e.clientY;
      const start = { x: r.left - b.left, y: r.top - b.top, w: r.width, h: r.height };
      rect.setPointerCapture(e.pointerId);
      const move = (m) => {
        const dx = m.clientX - sx, dy = m.clientY - sy;
        const n = resize
          ? { ...start, w: Math.max(40, start.w + dx), h: Math.max(16, start.h + dy) }
          : { ...start, x: start.x + dx, y: start.y + dy };
        const f = {
          x: Math.min(Math.max(n.x / b.width, 0), 1 - n.w / b.width),
          y: Math.min(Math.max(n.y / b.height, 0), 1 - n.h / b.height),
          w: Math.min(n.w / b.width, 1),
          h: Math.min(n.h / b.height, 1),
        };
        Object.assign(rect.style, { left: `${f.x * 100}%`, top: `${f.y * 100}%`, width: `${f.w * 100}%`, height: `${f.h * 100}%` });
        onChange?.(f);
      };
      const up = () => { rect.removeEventListener('pointermove', move); rect.removeEventListener('pointerup', up); };
      rect.addEventListener('pointermove', move);
      rect.addEventListener('pointerup', up);
    });
  }

  // A stand-in game frame (16:9) with a dialogue box at the bottom, drawn in CSS.
  const gameFrame = (line = LINES[0]) => `
    <div aria-hidden="true" style="position:absolute;inset:0;background:linear-gradient(180deg,oklch(42% 0.05 240) 0%,oklch(30% 0.04 230) 55%,oklch(22% 0.02 140) 100%)"></div>
    <div aria-hidden="true" style="position:absolute;left:8%;top:22%;width:14%;height:48%;background:oklch(26% 0.02 240);border-radius:4px"></div>
    <div aria-hidden="true" style="position:absolute;left:62%;top:30%;width:22%;height:40%;background:oklch(24% 0.02 220);border-radius:4px"></div>
    <div aria-hidden="true" style="position:absolute;left:47%;top:44%;width:4%;height:22%;background:oklch(60% 0.05 60);border-radius:9999px 9999px 4px 4px"></div>
    <div aria-hidden="true" style="position:absolute;left:12.5%;top:74.5%;width:75%;height:15%;background:linear-gradient(90deg,transparent,oklch(10% 0 0 / .65) 15%,oklch(10% 0 0 / .65) 85%,transparent)"></div>
    <div aria-hidden="true" style="position:absolute;left:12.5%;top:76%;width:75%;text-align:center;color:oklch(85% 0.08 80);font:600 1.9cqw/1.4 'IBM Plex Sans',sans-serif">Mara</div>
    <div aria-hidden="true" style="position:absolute;left:12.5%;top:80%;width:75%;text-align:center;color:#fff;font:400 1.7cqw/1.45 'IBM Plex Sans',sans-serif">${esc(line.en)}</div>`;

  return { LINES, PROFILES, STATES, STATE_LABEL, params, pick, go, esc, feed, switcher, dragRect, gameFrame };
})();
