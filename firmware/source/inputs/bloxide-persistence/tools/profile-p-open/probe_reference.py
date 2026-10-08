"""Raw loopback characterization of an unmodified XCPlite library and named fixture."""
import hashlib,json,pathlib,signal,socket,struct,subprocess,sys,time
hub=pathlib.Path(sys.argv[1]).resolve();records=[]
def sha(b):return hashlib.sha256(b).hexdigest()
def campaign(mode):
 out=hub/'artifacts'/('probe-'+mode);out.mkdir(exist_ok=False)
 run=out/'files';run.mkdir();steps=[];boot=0
 sock=socket.socket(socket.AF_INET,socket.SOCK_DGRAM);sock.bind(('127.0.0.1',0));port=sock.getsockname()[1];sock.close()
 def start():
  nonlocal boot
  boot+=1;log=(out/f'server-{boot}.log').open('wb');p=subprocess.Popen([str(hub/'artifacts'/mode/'reference'),str(port)],cwd=run,stdout=log,stderr=subprocess.STDOUT)
  for i in range(100):
   if 'FIXTURE_READY' in (out/f'server-{boot}.log').read_text():break
   if p.poll() is not None:raise AssertionError('reference startup failed')
   time.sleep(.05)
  else:raise AssertionError('reference startup timeout')
  s=socket.socket(socket.AF_INET,socket.SOCK_DGRAM);s.settimeout(2);s.connect(('127.0.0.1',port))
  return p,log,s
 def stop():
  s.close();p.send_signal(signal.SIGTERM);p.wait(timeout=5);log.close();assert p.returncode==0
 ctr=0
 def q(label,hexq,want=None):
  nonlocal ctr
  b=bytes.fromhex(hexq);frame=struct.pack('<HH',len(b),ctr)+b;ctr+=1;s.send(frame);wire=s.recv(2048);n,c=struct.unpack('<HH',wire[:4]);a=wire[4:];assert len(a)==n
  row=dict(label=label,boot=boot,request=hexq,response=a.hex(),tx_frame=frame.hex(),rx_frame=wire.hex());steps.append(row)
  (out/'exchanges.json').write_text(json.dumps(steps,indent=2)+'\n')
  if want is not None:
   expected=bytearray.fromhex(want); actual=bytearray(a)
   if b[0]==0xe5: actual[1]=0 # Upstream leaves reserved response byte stale.
   if b[0]==0xea: actual[1:3]=bytes(2)
   assert actual==expected,(label,a.hex(),want)
  return a
 def snap(label):
  fs=list(run.glob('*.bin'));assert len(fs)==1;data=fs[0].read_bytes();(out/(label+'.bin')).write_bytes(data);return data
 def addr(segment):
  a=q(f'address-{segment}',f'e800{segment:02x}0000');assert a[0]==255;return int.from_bytes(a[4:8],'little')
 def read(segment,address,want):
  q('mta-read', 'f6000000'+address.to_bytes(4,'little').hex(),'ff');return q(f'read-{segment}','f504','ff'+want)
 def write(segment,address,value):
  q('mta-write','f6000000'+address.to_bytes(4,'little').hex(),'ff');q(f'write-{segment}','f004'+value,'ff')
 p,log,s=start()
 try:
  q('connect','ff00');q('pag','e9');q('segment-info','e801010000');q('page0','e7000100');q('page1','e7000101');q('other-page0','e7000200')
  a,b=addr(1),addr(2);read(1,a,'e803f401');read(2,b,'d007fa00');initial=snap('00-initial')
  write(1,a,'b80bee02');write(2,b,'a00f5802');q('freeze-initial','e50001','ff0000');q('save-none','f9010000','ff');assert snap('01-no-selection')==initial
  q('select-led','e60101','ff');q('freeze-selected','e50001','ff0001');q('save-led','f9010000','ff');saved=snap('02-one-selected');assert saved!=initial
  q('status-after-sync-save','fd');q('disconnect','fe','ff')
 finally:stop()
 p,log,s=start()
 try:
  q('connect-restart','ff00');read(1,a,'e803f401');read(2,b,'d007fa00');q('freeze-after-restart','e50001','ff0000')
  write(1,a,'a00f5802');q('freeze-clear','e60001','ff');q('save-cleared','f9010000','ff');assert snap('03-cleared')==saved
  q('freeze-reenable','e60101','ff');q('save-reenabled','f9010000','ff');new=snap('04-reenabled');assert new!=saved

 finally:stop()
 p,log,s=start()
 try:
  q('connect-third','ff00');read(1,a,'a00f5802');read(2,b,'d007fa00')
  if mode=='default':
   write(1,a,'88136400');q('ecu-reference-xcp-working','eb010101','ff');q('ecu-page','ea0101','ff000001');q('xcp-page','ea0201','ff000000');read(1,a,'88136400')
   q('select-divergent','e60101','ff');q('verify-divergent-selected','e50001','ff0001');before=snap('05-before-divergent');q('save-divergent','f9010000','ff');assert snap('06-divergent')==before
  else:q('single-no-page-switch','eb010101','fe20')
  # Named local filesystem fault only, no source patch. Failure occurs before F9 positive response.
  q('select-for-sync-failure','e60101','ff');f=next(run.glob('*.bin'));backup=f.with_suffix('.preserved');f.rename(backup);f.mkdir()
  try:q('synchronous-open-failure','f9010000','fe24');q('status-after-sync-failure','fd')
  finally:f.rmdir();backup.rename(f)
 finally:stop()
 p,log,s=start()
 try:
  q('connect-fourth','ff00');read(1,a,'a00f5802');read(2,b,'d007fa00')
 finally:stop()
 rows=[dict(path=str(f.relative_to(out)),sha256=sha(f.read_bytes())) for f in sorted(out.rglob('*')) if f.is_file()]
 result=dict(mode=mode,scope='observed reference behavior only; not P endpoint/async/durable-flash evidence',boots=boot,exchanges=len(steps),artifacts=rows)
 (out/'result.json').write_text(json.dumps(result,indent=2)+'\n');return result
for mode in ['default','single']:records.append(campaign(mode))
(hub/'artifacts/reference-results.json').write_text(json.dumps(records,indent=2)+'\n')
print('reference campaigns passed',[(r['mode'],r['exchanges']) for r in records])
