// /setup · subtitle box (FR-REG-02 to 06): a still from the card source,
// a box to drag over it, live OCR of the box while it moves, and "ใช้กรอบนี้"
// to save it to the current profile and to OBS filter yokoten-crop.
(() => {
  const $ = (id) => document.getElementById(id);
  const frame = $('region-frame');
  const still = $('region-still');
  const rect = $('region-rect');
  const JSON_HEADERS = { 'content-type': 'application/json' };
  // Same floor as the server (core::region::MIN_SIDE).
  const MIN_SIDE = 0.01;
  const KEY_STEP = 0.005;

  let profileId;
  let saved = null; // box stored in the profile
  let box = null; // box on screen; null after "ลากใหม่" until one is drawn
  let size = null; // still size in px = OBS canvas
  let stillUrl = null;

  const clamp = (v, lo, hi) => Math.min(Math.max(v, lo), hi);
  // Inside the frame and at least MIN_SIDE a side, as the server keeps it.
  const fit = (b) => {
    const w = clamp(b.w, MIN_SIDE, 1);
    const h = clamp(b.h, MIN_SIDE, 1);
    return { x: clamp(b.x, 0, 1 - w), y: clamp(b.y, 0, 1 - h), w, h };
  };
  const same = (a, b) => a === b || (!!a && !!b && ['x', 'y', 'w', 'h'].every((k) => Math.abs(a[k] - b[k]) < 1e-6));

  function errorText(error, fallback) {
    if (!error) return fallback;
    if (typeof error === 'string') return error;
    return [error.message, error.fix].filter(Boolean).join(' · ');
  }

  // Show `text` in element `id`, coloured by `cls` (ok, warn, err or none).
  function say(id, text, cls) {
    const el = $(id);
    el.textContent = text ?? '';
    el.classList.remove('ok', 'warn', 'err');
    if (cls) el.classList.add(cls);
    el.hidden = !text;
  }

  function draw() {
    rect.hidden = !box || !size;
    if (box) {
      Object.assign(rect.style, {
        left: `${box.x * 100}%`, top: `${box.y * 100}%`, width: `${box.w * 100}%`, height: `${box.h * 100}%`,
      });
    }
    $('region-val').textContent = box && size
      ? `${Math.round(box.w * size.w)}×${Math.round(box.h * size.h)} px ที่ (${Math.round(box.x * size.w)}, ${Math.round(box.y * size.h)})`
      : '';
    $('region-apply').disabled = !box || !size;
    $('region-redraw').disabled = !size;
    $('region-cancel').disabled = same(box, saved);
  }

  // Live OCR of the box (FR-REG-02). One request at a time; a box that
  // changes meanwhile is read again as soon as the answer is in.
  let busy = false;
  let again = false;
  async function preview() {
    if (!box || !size) return;
    if (busy) {
      again = true;
      return;
    }
    busy = true;
    const res = await fetch('/api/ocr-preview', { method: 'POST', headers: JSON_HEADERS, body: JSON.stringify(box) }).catch(() => null);
    const body = await res?.json().catch(() => null);
    busy = false;
    if (res?.ok && body) {
      $('region-ocr').textContent = body.lines.length ? body.lines.join('\n') : '—';
      say('region-hint', body.hint ?? 'อ่านครบ', body.hint ? 'warn' : 'ok');
    } else {
      $('region-ocr').textContent = '—';
      say('region-hint', errorText(body?.error, 'อ่านกรอบนี้ไม่ได้ ลองอีกครั้ง'), 'err');
    }
    if (again) {
      again = false;
      preview();
    }
  }

  function changed() {
    draw();
    preview();
  }

  // Still JPG from the card (FR-REG-06), kept in browser memory only.
  async function takeStill() {
    $('region-shot').disabled = true;
    const res = await fetch('/api/obs/still', { cache: 'no-store' }).catch(() => null);
    if (res?.ok) {
      const url = URL.createObjectURL(await res.blob());
      still.src = url;
      await still.decode().catch(() => {});
      if (stillUrl) URL.revokeObjectURL(stillUrl);
      stillUrl = url;
      size = { w: still.naturalWidth, h: still.naturalHeight };
      frame.style.aspectRatio = `${size.w} / ${size.h}`;
      still.hidden = false;
      $('region-empty').hidden = true;
      say('region-result', '');
      changed();
    } else {
      const body = await res?.json().catch(() => null);
      say('region-result', errorText(body?.error, 'ต่อ Yokoten ไม่ได้ ลองอีกครั้ง'), 'err');
    }
    $('region-shot').disabled = false;
  }

  async function loadRegion() {
    const body = await fetch('/api/region').then((r) => r.json()).catch(() => null);
    saved = body?.region ?? null;
    box = saved;
    changed();
  }

  // Drag inside the box to move it, a corner to resize it, or the picture
  // around it to draw a new one. Fractions 0–1 of the still.
  frame.addEventListener('pointerdown', (e) => {
    if (!size || e.button !== 0) return;
    const area = frame.getBoundingClientRect();
    const at = (ev) => ({
      x: clamp((ev.clientX - area.left) / area.width, 0, 1),
      y: clamp((ev.clientY - area.top) / area.height, 0, 1),
    });
    const p0 = at(e);
    const start = box;
    const corner = e.target.dataset.corner;
    const mode = corner ?? (start && e.target.closest('#region-rect') ? 'move' : 'draw');
    // Resizing pins the opposite corner; drawing pins where the press began.
    const pin = mode === 'draw' || mode === 'move' ? p0 : {
      x: mode.includes('w') ? start.x + start.w : start.x,
      y: mode.includes('n') ? start.y + start.h : start.y,
    };
    // The corner being dragged follows the pointer's movement, so grabbing a
    // handle off-centre does not make the box jump.
    const grab = mode === 'draw' || mode === 'move' ? p0 : {
      x: mode.includes('w') ? start.x : start.x + start.w,
      y: mode.includes('n') ? start.y : start.y + start.h,
    };
    frame.setPointerCapture(e.pointerId);
    e.preventDefault();
    const move = (m) => {
      const p = at(m);
      const q = { x: clamp(grab.x + p.x - p0.x, 0, 1), y: clamp(grab.y + p.y - p0.y, 0, 1) };
      box = mode === 'move'
        ? fit({ ...start, x: start.x + p.x - p0.x, y: start.y + p.y - p0.y })
        : fit({ x: Math.min(pin.x, q.x), y: Math.min(pin.y, q.y), w: Math.abs(q.x - pin.x), h: Math.abs(q.y - pin.y) });
      changed();
    };
    const up = () => {
      frame.removeEventListener('pointermove', move);
      frame.removeEventListener('pointerup', up);
      frame.removeEventListener('pointercancel', up);
    };
    frame.addEventListener('pointermove', move);
    frame.addEventListener('pointerup', up);
    frame.addEventListener('pointercancel', up);
  });

  // Keyboard: arrows move the box, Shift + arrows resize it.
  rect.addEventListener('keydown', (e) => {
    const d = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] }[e.key];
    if (!d || !box) return;
    e.preventDefault();
    const [dx, dy] = d.map((v) => v * KEY_STEP);
    box = e.shiftKey ? fit({ ...box, w: box.w + dx, h: box.h + dy }) : fit({ ...box, x: box.x + dx, y: box.y + dy });
    changed();
  });

  function cancel() {
    box = saved;
    say('region-result', '');
    changed();
  }

  // FR-REG-03: ยกเลิก and Esc put back the saved box.
  $('region-cancel').onclick = cancel;
  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape' && !same(box, saved)) cancel();
  });

  $('region-redraw').onclick = () => {
    box = null;
    draw();
    $('region-ocr').textContent = '—';
    say('region-hint', 'ลากบนภาพเพื่อวาดกรอบใหม่', '');
  };

  $('region-shot').onclick = takeStill;

  // FR-REG-04, 06: save to the profile and set yokoten-crop to match.
  $('region-apply').onclick = async () => {
    $('region-apply').disabled = true;
    const res = await fetch('/api/region', { method: 'PUT', headers: JSON_HEADERS, body: JSON.stringify(box) }).catch(() => null);
    const body = await res?.json().catch(() => null);
    // The server keeps the box in the profile even when OBS cannot be reached.
    if (body?.region) {
      saved = body.region;
      box = saved;
    }
    const c = body?.crop;
    if (res?.ok && c) {
      say('region-result', `ใช้กรอบนี้แล้ว: yokoten-crop ตัด ${c.cx}×${c.cy} px ที่ (${c.left}, ${c.top})`, 'ok');
    } else if (res?.ok) {
      say('region-result', 'บันทึกกรอบในโปรไฟล์แล้ว', 'ok');
    } else if (body?.region) {
      say('region-result', `บันทึกกรอบในโปรไฟล์แล้ว แต่ตั้ง yokoten-crop ไม่ได้: ${errorText(body.error, '')}`, 'err');
    } else {
      say('region-result', errorText(body?.error, 'ต่อ Yokoten ไม่ได้ ลองอีกครั้ง'), 'err');
    }
    draw();
  };

  // The box belongs to the current profile: reload it when that changes.
  Yokoten.follow({
    state: (s) => {
      const id = s.profile?.id ?? null;
      if (id === profileId) return;
      profileId = id;
      loadRegion();
    },
  });

  draw();
  takeStill();
})();
