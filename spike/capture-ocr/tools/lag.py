# usage: lag.py fast_src delayed_src secs -> estimate delay by matching tiny screenshots over time
import json,base64,hashlib,os,asyncio,sys,time
import websockets
cfg=json.load(open(os.path.expanduser("~/Library/Application Support/obs-studio/plugin_config/obs-websocket/config.json")))
async def req(ws,t,d):
    await ws.send(json.dumps({"op":6,"d":{"requestType":t,"requestId":t,"requestData":d}}))
    while True:
        m=json.loads(await ws.recv())
        if m["op"]==7: return m["d"]
async def shot(ws,src):
    r=await req(ws,"GetSourceScreenshot",{"sourceName":src,"imageFormat":"bmp","imageWidth":48,"imageHeight":27})
    return base64.b64decode(r["responseData"]["imageData"].split(",")[1])[-48*27*4:]
async def main():
    fast,slow,secs=sys.argv[1],sys.argv[2],float(sys.argv[3])
    async with websockets.connect("ws://localhost:4455",subprotocols=["obswebsocket.json"],max_size=None) as ws:
        a=json.loads(await ws.recv())["d"]["authentication"]
        sec=base64.b64encode(hashlib.sha256((cfg["server_password"]+a["salt"]).encode()).digest()).decode()
        await ws.send(json.dumps({"op":1,"d":{"rpcVersion":1,"authentication":base64.b64encode(hashlib.sha256((sec+a["challenge"]).encode()).digest()).decode()}})); await ws.recv()
        F=[];D=[];t0=time.time();step=0.05
        while time.time()-t0<secs:
            F.append(await shot(ws,fast)); D.append(await shot(ws,slow))
            await asyncio.sleep(max(0,t0+len(F)*step-time.time()))
        n=len(F); period=(time.time()-t0)/n
        motion=sum(sum(abs(x-y) for x,y in zip(F[i],F[i+1])) for i in range(n-1))/(n-1)/len(F[0])
        best=[]
        for L in range(0,int(3.0/period)):
            m=n-L
            if m<20: break
            err=sum(sum(abs(x-y) for x,y in zip(F[i],D[i+L])) for i in range(0,m,2))/((m+1)//2)/len(F[0])
            best.append((err,L))
        best.sort()
        print(f"samples {n} period {period*1000:.0f} ms motion {motion:.2f}; best lags:",[(round(L*period*1000),round(e,2)) for e,L in best[:4]])
asyncio.run(main())
