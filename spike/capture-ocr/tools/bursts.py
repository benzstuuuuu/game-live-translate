# usage: bursts.py vid.txt mic.txt -> 1 kHz bursts lasting 0.2-0.45 s (the 300 ms beep) vs flash times
import re,sys
v=[float(re.search(r"host ([\d.]+)",l).group(1)) for l in open(sys.argv[1]) if "host" in l]
t=None;rows=[]
for l in open(sys.argv[2]):
    m=re.search(r"pts_time:([\d.]+)",l)
    if m:t=float(m.group(1));continue
    if "RMS_level" in l:
        x=l.split("=")[1].strip(); rows.append((t,-200 if "inf" in x else float(x)))
floor=sorted(x for _,x in rows)[len(rows)//2]
g=[]
for a,b in rows:
    if b>floor+12:
        if g and a-g[-1][1]<0.06: g[-1][1]=a; g[-1][2]=max(g[-1][2],b)
        else: g.append([a,a,b])
beeps=[x for x in g if 0.2<=x[1]-x[0]<=0.45]
t0=v[0] if v else rows[0][0]
print("flashes", [round(x-t0,2) for x in v])
print("all bursts", [(round(x[0]-t0,2),round(x[1]-x[0],2)) for x in g][:20])
print("beeps", [round(x[0]-t0,2) for x in beeps])
for f in v:
    if beeps:
        b=min(beeps,key=lambda x:abs(x[0]-f)); print(f"  beep - flash {1000*(b[0]-f):+6.0f} ms")
