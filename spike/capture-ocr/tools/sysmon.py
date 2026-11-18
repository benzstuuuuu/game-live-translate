"""Sample system + OBS load every N seconds for the full-stack run (#14).

usage: python3 tools/sysmon.py <seconds> [interval=5] > out/sysmon.tsv
columns: t, mem_free_pct, swap_mb, pageouts, swapouts, compressed_mb, thermal (0 nominal .. 3 critical),
         cpu/mem per group (obs, discord, ocr, lt), OBS active_fps, render_skipped, render_total, avg_render_ms
pageouts/swapouts/render_* are deltas since the previous sample.
"""
import asyncio, base64, hashlib, json, os, re, subprocess, sys, time

import websockets

SECS = int(sys.argv[1])
STEP = int(sys.argv[2]) if len(sys.argv) > 2 else 5
GROUPS = {"obs": ["OBS"], "discord": ["Discord"], "ocr": ["obs_ocr"], "lt": ["live_probe", "ffmpeg"]}
HERE = os.path.dirname(os.path.abspath(__file__))
THERM = os.path.join(HERE, "..", "out", "therm")


def sh(*cmd):
    return subprocess.run(cmd, capture_output=True, text=True).stdout


def vm():
    s = sh("vm_stat")
    page = int(re.search(r"page size of (\d+)", s).group(1))
    get = lambda k: int(re.search(rf"{k}:\s+(\d+)", s).group(1))
    return get("Pageouts"), get("Swapouts"), get("Pages occupied by compressor") * page / 2**20


def procs():
    """cpu from ps (decaying average); memory from top's MEM (footprint), since ps rss misses compressed/GPU memory."""
    out = {g: [0.0, 0.0] for g in GROUPS}
    group = lambda name: next((g for g, keys in GROUPS.items() if any(k in name for k in keys)), None)
    for line in sh("ps", "-A", "-o", "%cpu=,comm=").splitlines():
        cpu, comm = line.split(None, 1)
        if g := group(comm):
            out[g][0] += float(cpu)
    unit = {"B": 2**-20, "K": 2**-10, "M": 1, "G": 2**10}
    for line in sh("top", "-l", "1", "-stats", "mem,command").splitlines():
        m = re.match(r"\s*([\d.]+)([BKMG])\+?-?\s+(.+)", line)
        if m and (g := group(m.group(3))):
            out[g][1] += float(m.group(1)) * unit[m.group(2)]
    return out


async def obs_conn():
    cfg = json.load(open(os.path.expanduser("~/Library/Application Support/obs-studio/plugin_config/obs-websocket/config.json")))
    ws = await websockets.connect("ws://localhost:4455", subprotocols=["obswebsocket.json"])
    hello = json.loads(await ws.recv())["d"]
    ident = {"rpcVersion": 1}
    if "authentication" in hello:
        a = hello["authentication"]
        sec = base64.b64encode(hashlib.sha256((cfg["server_password"] + a["salt"]).encode()).digest()).decode()
        ident["authentication"] = base64.b64encode(hashlib.sha256((sec + a["challenge"]).encode()).digest()).decode()
    await ws.send(json.dumps({"op": 1, "d": ident}))
    await ws.recv()
    return ws


async def obs_stats(ws):
    await ws.send(json.dumps({"op": 6, "d": {"requestType": "GetStats", "requestId": "s"}}))
    while True:
        m = json.loads(await ws.recv())
        if m["op"] == 7:
            return m["d"]["responseData"]


async def main():
    if not os.path.exists(THERM):
        src = os.path.join(HERE, "therm.swift")
        subprocess.run(["swiftc", "-O", "-o", THERM, src], check=True)
    ws = await obs_conn()
    head = ["t", "mem_free_pct", "swap_mb", "pageouts", "swapouts", "compressed_mb", "thermal"]
    head += [f"{g}_{k}" for g in GROUPS for k in ("cpu", "mem_mb")]
    head += ["obs_fps", "render_skipped", "render_total", "avg_render_ms"]
    print("\t".join(head), flush=True)
    t0 = time.time()
    po0, so0, _ = vm()
    s0 = await obs_stats(ws)
    while time.time() - t0 < SECS:
        await asyncio.sleep(STEP)
        free = int(re.search(r"(\d+)%", sh("memory_pressure", "-Q")).group(1))
        swap = float(re.search(r"used = ([\d.]+)M", sh("sysctl", "vm.swapusage")).group(1))
        po, so, comp = vm()
        therm = sh(THERM).strip()
        p = procs()
        s1 = await obs_stats(ws)
        row = [f"{time.time() - t0:.0f}", free, f"{swap:.0f}", po - po0, so - so0, f"{comp:.0f}", therm]
        row += [f"{p[g][i]:.{1 - i}f}" for g in GROUPS for i in (0, 1)]
        row += [f"{s1['activeFps']:.1f}", s1["renderSkippedFrames"] - s0["renderSkippedFrames"],
                s1["renderTotalFrames"] - s0["renderTotalFrames"], f"{s1['averageFrameRenderTime']:.2f}"]
        print("\t".join(map(str, row)), flush=True)
        po0, so0, s0 = po, so, s1


asyncio.run(main())
