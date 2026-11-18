// /setup · stream bar (FR-STR-03): where the Thai bar sits on the 1920x1080
// stream, kept per profile. Every slider move is saved and /overlay follows
// at once through the SSE event `bar`.
(() => {
  const $ = (id) => document.getElementById(id);
  // Same as the server (core::bar): two lines of 88 px, a fifth of the width.
  const HEIGHT = 176 / 1080;
  const MIN_W = 0.2;
  const W = 1920;
  const H = 1080;

  const clamp = (v, lo, hi) => Math.min(Math.max(v, lo), hi);
  const fit = (b) => {
    const w = clamp(b.w, MIN_W, 1);
    return { x: clamp(b.x, 0, 1 - w), y: clamp(b.y, 0, 1 - HEIGHT), w };
  };

  let bar = null;
  let profileId;

  function say(text, cls) {
    const el = $('bar-msg');
    el.textContent = text ?? '';
    el.className = cls ?? '';
    el.hidden = !text;
  }

  function draw() {
    const ready = !!bar;
    ['bar-x', 'bar-y', 'bar-w'].forEach((id) => { $(id).disabled = !ready; });
    $('bar-rect').hidden = !ready;
    if (!ready) return;
    Object.assign($('bar-rect').style, {
      left: `${bar.x * 100}%`, top: `${bar.y * 100}%`, width: `${bar.w * 100}%`, height: `${HEIGHT * 100}%`,
    });
    $('bar-x').max = String(1 - bar.w);
    $('bar-y').max = String(1 - HEIGHT);
    $('bar-x').value = String(bar.x);
    $('bar-y').value = String(bar.y);
    $('bar-w').value = String(bar.w);
    $('bar-x-val').textContent = `${Math.round(bar.x * W)} px`;
    $('bar-y-val').textContent = `${Math.round(bar.y * H)} px`;
    $('bar-w-val').textContent = `${Math.round(bar.w * W)} px`;
  }

  function drawRegion(r) {
    $('bar-region').hidden = !r;
    if (r) {
      Object.assign($('bar-region').style, {
        left: `${r.x * 100}%`, top: `${r.y * 100}%`, width: `${r.w * 100}%`, height: `${r.h * 100}%`,
      });
    }
  }

  async function load() {
    const [b, r] = await Promise.all([
      fetch('/api/bar').then((res) => (res.ok ? res.json() : null)).catch(() => null),
      fetch('/api/region').then((res) => (res.ok ? res.json() : null)).catch(() => null),
    ]);
    bar = b;
    drawRegion(r?.region ?? null);
    draw();
    say(b ? '' : 'โหลดตำแหน่งแถบไม่ได้ ลองเปิดหน้านี้ใหม่', b ? null : 'err');
  }

  // One PUT at a time; a bar that moves meanwhile is sent again right after.
  let busy = false;
  let again = false;
  async function save() {
    if (busy) {
      again = true;
      return;
    }
    busy = true;
    const res = await fetch('/api/bar', {
      method: 'PUT',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(bar),
    }).catch(() => null);
    const body = await res?.json().catch(() => null);
    busy = false;
    if (res?.ok) say('');
    else say(body?.error?.message ?? 'บันทึกตำแหน่งแถบไม่สำเร็จ ลองอีกครั้ง', 'err');
    if (again) {
      again = false;
      save();
    }
  }

  ['x', 'y', 'w'].forEach((k) => {
    $(`bar-${k}`).addEventListener('input', (e) => {
      if (!bar) return;
      bar = fit({ ...bar, [k]: Number(e.target.value) });
      draw();
      save();
    });
  });

  Yokoten.follow({
    state: (s) => {
      const id = s.profile?.id ?? null;
      if (id !== profileId) {
        profileId = id;
        load();
      }
    },
  });
  draw();
})();
