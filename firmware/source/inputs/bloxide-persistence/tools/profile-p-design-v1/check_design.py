#!/usr/bin/env python3
"""Validate candidate byte/schema/trace constraints. Does not emulate an endpoint."""
import copy,hashlib,json,pathlib,struct
ROOT=pathlib.Path(__file__).resolve().parents[2];SPEC=ROOT/'specs/profile-p-design-v1';EV=ROOT/'evidence/profile-p-design-v1'

def decode(b):
    assert len(b)==640 and b[:8]==b'BLXPVW01'
    assert struct.unpack_from('<HH',b,8)==(1,640)
    flags,=struct.unpack_from('<I',b,12); assert flags & ~15 == 0
    state,=struct.unpack_from('<I',b,36);assert state in range(6)
    reason,=struct.unpack_from('<I',b,52);assert reason in range(13)
    assert b[108:112]==bytes(4) and b[122:128]==bytes(6)
    assert b[120] in (0,1) and b[121] in (0,1)
    key=struct.unpack_from('<Q',b,40)[0],struct.unpack_from('<I',b,48)[0],struct.unpack_from('<Q',b,56)[0]
    if flags & 1: assert key[0]>0 and key[2]>0
    else: assert key==(0,0,0)
    assert bool(flags&1)==bool(flags&2)
    payload=[]
    for valid,meta,start in [(flags&2,64,128),(flags&4,88,384)]:
        epoch,rev,schema,length=struct.unpack_from('<QIHH',b,meta)
        if valid: assert 1<=length<=256 and schema>0
        else: assert (epoch,rev,schema,length)==(0,0,0,0)
        assert b[start+length:start+256]==b'\xff'*(256-length)
        payload.append(b[start:start+length])
    if not flags&4: assert b[80:120]==bytes(40)
    else: assert struct.unpack_from('<Q',b,80)[0]>0
    if state in (1,2,3,4):assert flags&3==3
    if state==1:assert not flags&8
    if state in (2,3):assert flags&8
    if state==2:assert reason==0
    conn,=struct.unpack_from('<I',b,32)
    if flags&1 and conn==key[1]: assert b[120]==int(state in (1,3,4))
    return dict(flags=flags,state=state,key=key,capture=payload[0],durable=payload[1],status=b[120],connection=conn)

def validate_descriptor(d):
    assert d['schema_version']==1 and d['status']=='candidate_design_only'
    assert d['release_eligible'] is False
    assert d['byte_order']=='little' and d['address_extension']==0
    assert d['max_cto']==8 and d['max_dto']==8 and d['max_clients']==4
    blocks=d['blocks'];assert len(blocks)==2
    ranges=[]
    for b in blocks:
        assert type(b['address']) is int and type(b['size']) is int
        assert 0<=b['address']<2**32 and 0<b['size']<=640
        assert b['address']+b['size']<=2**32 and b['access']=='read_only'
        ranges.append((b['address'],b['address']+b['size']))
    assert ranges[0][1]<=ranges[1][0] or ranges[1][1]<=ranges[0][0]
    assert [(b['name'],b['address'],b['size']) for b in blocks]==[('persistence_view',65536,640),('persistence_identity',66560,128)]
    assert blocks[0]['latch']=='SET_MTA_at_base' and blocks[1]['latch']=='immutable'
    st=d['storage'];assert sum(v for k,v in st.items() if k not in ('total','ceiling'))==st['total']==5120
    assert st['total']<=st['ceiling']==6144
    assert d['calibration']==dict(address=4096,size=4,schema_id=1,segment=0,page=0,init_segment=0,init_segment_review='application_policy_pending_independent_review')
    return True

def main():
    d=json.loads((SPEC/'vectors.json').read_text()); assert d['scope']=='candidate_design_only_not_endpoint_acceptance'
    descriptor=json.loads((SPEC/'descriptor.json').read_text());validate_descriptor(descriptor)
    assert descriptor['blocks'][0]['fields']==d['fields']
    descriptor_bad=[]
    for field,value in [('address',65536),('size',0),('access','read_write')]:
        bad=copy.deepcopy(descriptor);bad['blocks'][1][field]=value;descriptor_bad.append(bad)
    for field,value in [('release_eligible',True),('max_clients',5),('address_extension',1)]:
        bad=copy.deepcopy(descriptor);bad[field]=value;descriptor_bad.append(bad)
    bad=copy.deepcopy(descriptor);bad['storage']['summaries']=0;descriptor_bad.append(bad)
    for bad in descriptor_bad:
        try:validate_descriptor(bad)
        except AssertionError:pass
        else:raise AssertionError('invalid descriptor accepted')
    coverage=[]
    for f in d['fields']:coverage+=list(range(f['offset'],f['offset']+f['size']))
    assert coverage==list(range(640)), 'schema gaps/overlap'
    views={}; decoded={};checks=0
    for v in d['views']:
        b=(EV/v['file']).read_bytes();views[v['name']]=b;decoded[v['name']]=decode(b)
        for f in d['fields']:
            actual=b[f['offset']:f['offset']+f['size']];expected=v['expected'].get(f['name'],0)
            if isinstance(expected,str):assert actual.hex()==expected
            else:assert int.from_bytes(actual,'little')==expected
            checks+=1
        # Every permitted chunk width must reconstruct the same literal fixture.
        for width in range(1,8):assert b''.join(b[i:i+width] for i in range(0,640,width))==b
    assert views['pending'][128:132].hex()=='b80bee02'
    assert struct.unpack_from('<HH',views['pending'],128)==(3000,750)
    assert struct.unpack_from('<HH',views['pending'],384)==(2000,250)
    assert decoded['durable']['capture']==decoded['durable']['durable']
    assert decoded['failed']['durable']==decoded['pending']['durable']
    assert decoded['indeterminate']['flags']&8==0
    assert decoded['pending']['capture'].hex()!=d['later_ram_hex']
    assert decoded['reconnect_failed']['key']==decoded['failed']['key']
    assert decoded['reconnect_failed']['connection']!=decoded['failed']['connection']
    assert decoded['reboot']['flags']==4 and decoded['reboot']['key']==(0,0,0)
    assert decoded['reboot']['capture']==b''
    assert decoded['reboot']['durable']==decoded['durable']['durable']
    assert decoded['durable_existing']['key'][2]==10
    assert struct.unpack_from('<Q',views['durable_existing'],112)[0]==9
    for t in d['traces']:
        assert [decoded[n]['status'] for n in t['steps']]==t['status']
        # Only live connection transitions preserve the full retained key.
        if 'reboot' not in t['steps']: assert len({decoded[n]['key'] for n in t['steps']})==1
    # Deliberate corruption controls must fail, including false uncertainty release.
    bad=[]
    for off,value in [(0,0),(8,2),(10,0),(12,0x87),(36,6),(52,13),(108,1),(122,1),(120,0),(132,0),(388,0)]:
        b=bytearray(views['pending']);b[off]=value;bad.append(bytes(b))
    b=bytearray(views['pending']);b[78:80]=(257).to_bytes(2,'little');bad.append(bytes(b))
    b=bytearray(views['pending']);b[12]|=8;bad.append(bytes(b))
    b=bytearray(views['durable']);b[12]&=~8;bad.append(bytes(b))
    bad.extend([views['durable'][:-1],views['durable']+b'\0'])
    for b in bad:
        try:decode(b)
        except AssertionError:pass
        else:raise AssertionError('malformed vector accepted')
    c={x['id']:x for x in d['commands']};assert len(c)==len(d['commands'])
    for x in c.values():
        q=bytes.fromhex(x['request']);a=bytes.fromhex(x['response']) if x['response'] else b''
        assert 1<=len(q)<=8 and len(a)<=8
        if a:assert a[0] in (0xfe,0xff)
        if a[:1]==b'\xfe':assert len(a)==2 and a[1]!=255
    expected={'admit':'ff','s0-unknown':'fe20','short':'fe21','long':'fe21','opcode-only':'fe21','freeze-off':'fe27','stopped':'fe27','busy':'fe10','full':'fe10','rate':'fe10','known-write-lock':'fe24','schema':'fe24','quarantine':'fe24','known-q-lock':'fe24','deadline':'fe31','bad-segment':'fe28','bad-page':'fe26','bad-mode':'fe27','bad-reserved':'fe21','bad-freeze':'fe22'}
    for n,res in expected.items():assert c[n]['response']==res
    assert sum(bool(x.get('capture')) for x in c.values())==1
    for n,x in c.items():
        if n.startswith(('mode-','id-')):assert x['response']=='fe22' and not x['capture']
    assert c['admit']['request']=='f9010000' and c['disconnected']['response'] is None
    from check_admission import check_all
    correction = check_all(ROOT)
    identity=(EV/'identity.bin').read_bytes();assert len(identity)==128 and identity[:8]==b'BLXPP001'
    assert struct.unpack_from('<HHI',identity,8)==(1,640,0x50000001)
    assert struct.unpack_from('<IIHHBBBB',identity,112)==(0x10000,0x1000,4,1,4,1,1,0)
    assert identity[16:112]==bytes.fromhex('11'*32+'22'*32+'33'*32)
    assert 4*640+4*384+640+128+4*32+128==5120<=6144
    out={'scope':'design constraints only; no endpoint/interoperability or target evidence','views':len(views),'field_checks':checks,'negative_schema_controls':len(bad),'descriptor_negative_controls':len(descriptor_bad),'wire_vectors':len(c),'traces':len(d['traces']),'max_view_uploads':(640+6)//7,'max_identity_uploads':(128+6)//7,'declared_extra_bytes':5120,'vector_sha256':hashlib.sha256((SPEC/'vectors.json').read_bytes()).hexdigest()}
    out['admission_correction']=correction
    print(json.dumps(out,indent=2))
if __name__=='__main__':main()
