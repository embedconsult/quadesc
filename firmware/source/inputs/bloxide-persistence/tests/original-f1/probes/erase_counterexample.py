"""Bounded algebraic witness, NOT a persistence product implementation.
Find a legal 0->1 erase subset mapping one valid R2 codeword to a DIFFERENT
valid codeword with a DIFFERENT CRC, and demonstrate R3 selection.
"""
import struct,json,hashlib
from pathlib import Path
R=Path(__file__).resolve().parents[1]
POLY=0x82f63b78

def crc(data):
 c=0xffffffff
 for b in data:
  c^=b
  for _ in range(8): c=(c>>1)^(POLY if c&1 else 0)
 return c^0xffffffff

def crc_forward(data):
 # Independent non-reflected bit-by-bit polynomial check with reversed input/output.
 c=0xffffffff
 for b in data:
  c ^= int(f'{b:08b}'[::-1],2)<<24
  for _ in range(8):c=((c<<1)^(0x1edc6f41 if c&0x80000000 else 0))&0xffffffff
 return int(f'{c:032b}'[::-1],2)^0xffffffff

def domain(b):return b[:60]+b[64:320]
def record(seq,period,duty,revision=9):
 b=bytearray(b'\xff'*512);b[:8]=b'BLXPERS1'
 struct.pack_into('<HHIQQIIQ',b,8,1,1,4,seq,1,revision,1,seq)
 b[48:60]=bytes(12);struct.pack_into('<HH',b,64,period,duty)
 struct.pack_into('<I',b,60,crc(domain(b)));b[320:328]=bytes(8)
 return b

def validate(b):
 assert len(b)==512 and b[:8]==b'BLXPERS1'
 fmt,schema,n,seq,epoch,rev,generation,op=struct.unpack_from('<HHIQQIIQ',b,8)
 assert (fmt,schema,n)==(1,1,4) and seq>0
 assert b[48:60]==bytes(12) and b[68:320]==b'\xff'*252
 assert b[320:328]==bytes(8) and b[328:]==b'\xff'*184
 stored=struct.unpack_from('<I',b,60)[0]
 assert crc(domain(b))==crc_forward(domain(b))==stored
 period,duty=struct.unpack_from('<HH',b,64);assert 100<=period<=10000 and duty<=1000
 return {'record_sequence':seq,'source_epoch':epoch,'source_revision':rev,'period':period,'duty':duty,'crc32c':f'{stored:08x}'}

assert crc(b'123456789')==crc_forward(b'123456789')==0xe3069283
old=record(6,1000,500);active=record(7,2000,250,10);requested=record(8,3000,750,11)
changed=bytearray(old);changed[16]|=8 # seq6 -> 14; only 0->1
original_crc=crc(domain(old));wanted_crc=original_crc|((~original_crc & 0xffffffff)&-(~original_crc & 0xffffffff))
assert original_crc != wanted_crc
baseline_crc=crc(domain(changed));variables=[i for i in range(24*8,32*8) if not changed[i//8]>>(i%8)&1]
basis={}
for j,bit in enumerate(variables):
 trial=bytearray(changed);trial[bit//8]|=1<<(bit%8)
 vector=crc(domain(trial))^baseline_crc;selection=1<<j
 while vector:
  k=vector.bit_length()-1
  if k not in basis:basis[k]=(vector,selection);break
  vector^=basis[k][0];selection^=basis[k][1]
vector=wanted_crc^baseline_crc;selection=0
while vector:
 k=vector.bit_length()-1;assert k in basis
 vector^=basis[k][0];selection^=basis[k][1]
for j,bit in enumerate(variables):
 if selection>>j&1:changed[bit//8]|=1<<(bit%8)
struct.pack_into('<I',changed,60,wanted_crc)
assert all((x|y)==y for x,y in zip(old,changed)), 'must be achievable by erase-only 0->1 transitions'
a,b,c,n=map(validate,(active,old,changed,requested))
assert c['record_sequence']>a['record_sequence']
assert (c['period'],c['duty']) not in [(x['period'],x['duty']) for x in (a,n)]
assert original_crc!=wanted_crc and domain(old)!=domain(changed)
files={}
for name,data in [('active-a.bin',active),('inactive-b-before.bin',old),('inactive-b-torn-erase.bin',changed),('intended-new.bin',requested),('erase-mask.bin',bytes(x^y for x,y in zip(old,changed)))]:
 path=R/'logs'/name;path.write_bytes(data);files[name]=hashlib.sha256(data).hexdigest()
changes=[{'offset':i,'before':f'{x:02x}','after':f'{y:02x}','mask':f'{x^y:02x}'} for i,(x,y) in enumerate(zip(old,changed)) if x!=y]
out={'scope':'design algebraic counterexample only','G':8,'E':512,'basis_rank':len(basis),'active':a,'inactive_before':b,'inactive_after_torn_erase':c,'requested_new':n,'crc_collision':False,'only_zero_to_one':True,'marker_untouched':changed[320:328]==old[320:328],'other_slot_untouched':True,'changed_bits':sum((x^y).bit_count() for x,y in zip(old,changed)),'changed_bytes':changes,'recovery_by_R3':'B seq14, period1000/duty500; neither selected old nor captured new','artifacts_sha256':files}
(R/'logs/erase-counterexample.json').write_text(json.dumps(out,indent=2)+'\n');print(json.dumps(out,indent=2))
