# usage: shots.py src1 src2 ... -> mean luma + frame-difference of two consecutive tiny screenshots per source
import json,base64,hashlib,os,asyncio,sys,io,time
import websockets
cfg=json.load(open(os.path.expanduser("~/Library/Application Support/obs-studio/plugin_config/obs-websocket/config.json")))
async def req(ws,t,d):
    await ws.send(json.dumps({"op":6,"d":{"requestType":t,"requestId":t,"requestData":d}}))
    while True:
        m=json.loads(await ws.recv())
        if m["op"]==7: return m["d"]
async def main():
    async with websockets.connect("ws://localhost:4455",subprotocols=["obswebsocket.json"]) as ws:
        a=json.loads(await ws.recv())["d"]["authentication"]
        sec=base64.b64encode(hashlib.sha256((cfg["server_password"]+a["salt"]).encode()).digest()).decode()
        await ws.send(json.dumps({"op":1,"d":{"rpcVersion":1,"authentication":base64.b64encode(hashlib.sha256((sec+a["challenge"]).encode()).digest()).decode()}})); await ws.recv()
        for src in sys.argv[1:]:
            imgs=[]
            for _ in range(2):
                r=await req(ws,"GetSourceScreenshot",{"sourceName":src,"imageFormat":"bmp","imageWidth":64,"imageHeight":36})
                if not r["requestStatus"]["result"]: print(src,r["requestStatus"]); break
                imgs.append(base64.b64decode(r["responseData"]["imageData"].split(",")[1])[-64*36*4:]); time.sleep(0.5)
            if len(imgs)==2:
                luma=sum(imgs[1][1::4])/(64*36); diff=sum(abs(x-y) for x,y in zip(imgs[0],imgs[1]))/len(imgs[0])
                print(f"{src}: mean green {luma:.0f}  change over 0.5s {diff:.1f}")
asyncio.run(asyncio.wait_for(main(),20))
