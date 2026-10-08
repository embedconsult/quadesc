"""Independent parsed-field check for P metadata using the unchanged pinned parser."""
import importlib.metadata,json,pathlib,sys
import pya2l
from pya2l import model
root=pathlib.Path(__file__).resolve().parents[2];out=pathlib.Path(sys.argv[1]);out.mkdir(exist_ok=False)
assert importlib.metadata.version('pya2ldb')=='1.0.353'
def check(text,name):
 directory=out/name;directory.mkdir();f=directory/'candidate.a2l';f.write_text(text)
 db=pya2l.import_a2l(str(f),in_memory=True,progress_bar=False,output_dir=directory)
 segs=db.query(model.MemorySegment).all();assert len(segs)==1,'segment_count'
 s=segs[0];assert (s.name,s.prgType,s.memoryType,s.attribute,s.address,s.size)==('led_page','DATA','RAM','INTERN',4096,4),'segment_map'
 assert [getattr(s,'offset_'+str(i)) for i in range(5)]==[-1]*5,'mirrors'
 values={c.name:(c.address,c.lowerLimit,c.upperLimit,c.type,c.deposit,c.ecu_address_extension.extension) for c in db.query(model.Characteristic).all()}
 assert values=={'led_period_ms':(4096,100,10000,'VALUE','scalar_u16',0),'led_duty_permille':(4098,0,1000,'VALUE','scalar_u16',0)},'scalars'
 blobs={b.name:(b.address,b.size,b.calibration_access.type) for b in db.query(model.Blob).all()}
 assert blobs=={'persistence_view':(65536,640,'NO_CALIBRATION'),'persistence_identity':(66560,128,'NO_CALIBRATION')},'readonly_blobs'
 assert '/begin IF_DATA' not in text,'no_auto_discovery'
 return dict(segment=[s.name,s.address,s.size],scalars=values,blobs=blobs)
text=(root/'specs/profile-p-design-v1/metadata.a2l').read_text();positive=check(text,'valid');negative=[]
for i,(old,new,reason) in enumerate([('DATA RAM INTERN 0x1000 4','DATA RAM INTERN 0x1000 8','segment_map'),('VALUE 0x1002','VALUE 0x1004','scalars'),('100 10000','99 10000','scalars'),('0 NO_COMPU_METHOD 0 1000','0 NO_COMPU_METHOD 0 1001','scalars'),('CALIBRATION_ACCESS NO_CALIBRATION','CALIBRATION_ACCESS CALIBRATION','readonly_blobs')]):
 assert old in text
 try:check(text.replace(old,new),f'mutation-{i}')
 except AssertionError as e:assert str(e)==reason;negative.append(dict(change=new,rejected_at=reason))
 else:raise AssertionError('bad metadata accepted')
print(json.dumps(dict(scope='metadata syntax/semantic fields only; no endpoint',positive=positive,negative_controls=negative),indent=2))
