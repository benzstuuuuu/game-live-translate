import json,base64,hashlib,os,asyncio,sys
import websockets
cfg=json.load(open(os.path.expanduser("~/Library/Application Support/obs-studio/plugin_config/obs-websocket/config.json")))
async def main():
    async with websockets.connect("ws://localhost:4455",subprotocols=["obswebsocket.json"]) as ws:
        hello=json.loads(await ws.recv())["d"]; ident={"rpcVersion":1}
        if "authentication" in hello:
            a=hello["authentication"]
            sec=base64.b64encode(hashlib.sha256((cfg["server_password"]+a["salt"]).encode()).digest()).decode()
            ident["authentication"]=base64.b64encode(hashlib.sha256((sec+a["challenge"]).encode()).digest()).decode()
        await ws.send(json.dumps({"op":1,"d":ident})); await ws.recv()
        for t,d in json.loads(sys.argv[1]):
            await ws.send(json.dumps({"op":6,"d":{"requestType":t,"requestId":t,"requestData":d}}))
            while True:
                m=json.loads(await ws.recv())
                if m["op"]==7: r=m["d"]; break
            print(t, r["requestStatus"], json.dumps({k:v for k,v in (r.get("responseData") or {}).items() if k!="availableRequests"},ensure_ascii=False)[:600])
asyncio.run(asyncio.wait_for(main(),15))
