#!/usr/bin/env python3
"""Dedicated unchanged pyXCP/pya2ldb PTY exercise of the actual host P service."""
from __future__ import annotations
import argparse, hashlib, importlib.metadata as md, json, os, pathlib, struct
import pya2l
from pya2l import model
from xcp_s0_interop.capture import Capture, environment_metadata
from xcp_s0_interop.client import CapturingSerial, build_application, read_bytes
from xcp_s0_interop.p_transport import PSerialTransport
from pyxcp import Master
from xcp_s0_interop.profile import EXTERNAL_19200_SXI
from product_endpoint_pty import EndpointDriver, PtyBridge

ROOT=pathlib.Path(__file__).parents[1]
FIXTURE=ROOT/'fixtures/profile-p-host'

def sha(b:bytes)->bytes:return hashlib.sha256(b).digest()
def pair(a:bytes,b:bytes)->bytes:return sha(b'P-DESCRIPTOR-A2L-v1'+struct.pack('<I',len(a))+a+struct.pack('<I',len(b))+b)
def parse_metadata(output:pathlib.Path)->dict:
    path=FIXTURE/'metadata.a2l';text=path.read_text();db=pya2l.import_a2l(str(path),in_memory=True,progress_bar=False,output_dir=output)
    try:
        common=db.query(model.ModCommon).one();assert common.byte_order.byteOrder=='MSB_LAST'
        layout=db.query(model.RecordLayout).one();assert (layout.name,layout.fnc_values.datatype)==('scalar_u16','UWORD')
        seg=db.query(model.MemorySegment).one();assert (seg.prgType,seg.memoryType,seg.attribute,seg.address,seg.size)==('DATA','RAM','INTERN',0x1000,4)
        chars={c.name:(c.address,c.lowerLimit,c.upperLimit,c.type,c.deposit,c.ecu_address_extension.extension) for c in db.query(model.Characteristic).all()}
        assert chars=={'led_period_ms':(0x1000,100,10000,'VALUE','scalar_u16',0),'led_duty_permille':(0x1002,0,1000,'VALUE','scalar_u16',0)}
        blobs={x.name:(x.address,x.size,x.calibration_access.type) for x in db.query(model.Blob).all()}
        assert blobs=={'persistence_view':(0x10000,640,'NO_CALIBRATION'),'persistence_identity':(0x10400,128,'NO_CALIBRATION')}
        assert '/begin IF_DATA' not in text
        return {'byte_order':'MSB_LAST','layout':'UWORD','segment':seg.name,'scalars':chars,'blobs':blobs}
    finally:db.close()

def make_bundle(endpoint:pathlib.Path,output:pathlib.Path)->tuple[bytes,dict]:
    desc=(FIXTURE/'descriptor.json').read_bytes();a2l=(FIXTURE/'metadata.a2l').read_bytes();policy=(FIXTURE/'policy.json').read_bytes()
    image=endpoint.read_bytes();data=bytearray(128);data[:8]=b'BLXPP001';struct.pack_into('<HHI',data,8,1,640,0x50000001)
    data[16:48]=sha(image);data[48:80]=pair(desc,a2l);data[80:112]=sha(policy)
    struct.pack_into('<IIHHBBBB',data,112,0x10000,0x1000,4,1,4,1,1,0)
    identity=bytes(data);(output/'identity.bin').write_bytes(identity)
    manifest={'scope':'host_fixture_only','production_image':False,'image_sha256':sha(image).hex(),'descriptor_a2l_sha256':pair(desc,a2l).hex(),'policy_sha256':sha(policy).hex(),'identity_sha256':sha(identity).hex(),'source_pins':json.loads((FIXTURE/'source-pins.json').read_text())}
    (output/'bundle.json').write_text(json.dumps(manifest,indent=2)+'\n')
    return identity,manifest

def admit_identity(observed:bytes,expected:bytes,manifest:dict,endpoint:pathlib.Path,*,allow_host_fixture:bool=False)->None:
    if len(observed)!=128 or observed[:8]!=b'BLXPP001' or struct.unpack_from('<HHI',observed,8)!=(1,640,0x50000001):raise ValueError('not matched P identity')
    if not allow_host_fixture and (manifest.get('scope')!='production' or not manifest.get('production_image')):raise ValueError('host fixture cannot authorize production writes')
    if manifest.get('scope') not in ('production','host_fixture_only'):raise ValueError('unknown bundle scope')
    if observed!=expected or observed[16:48]!=sha(endpoint.read_bytes()) or observed[48:80]!=bytes.fromhex(manifest['descriptor_a2l_sha256']) or observed[80:112]!=bytes.fromhex(manifest['policy_sha256']):raise ValueError('stale or mismatched identity')
    if observed[16:112]==b'\0'*96 or observed[16:112]==b'\xcd'*96:raise ValueError('synthetic identity')
    if struct.unpack_from('<IIHHBBBB',observed,112)!=(0x10000,0x1000,4,1,4,1,1,0):raise ValueError('identity layout mismatch')

def negatives(expected:bytes,manifest:dict,endpoint:pathlib.Path)->list[str]:
    bad=[]
    controls={'s0':b'BLXS0001'+expected[8:],'synthetic':expected[:16]+b'\xcd'*96+expected[112:],'stale':expected[:16]+b'\x01'*32+expected[48:],'mismatch':expected[:-1]+b'\x01'}
    for name,identity in controls.items():
        try:admit_identity(identity,expected,manifest,endpoint,allow_host_fixture=True)
        except ValueError:bad.append(name)
        else:raise AssertionError(f'{name} passed identity admission')
    try:admit_identity(expected,expected,manifest,endpoint)
    except ValueError:bad.append('host_fixture_as_production')
    else:raise AssertionError('host fixture passed production admission')
    return bad

def run(endpoint:pathlib.Path,output:pathlib.Path)->dict:
    assert md.version('pyxcp')=='0.29.18' and md.version('pya2ldb')=='1.0.353'
    capture=Capture(output);metadata=parse_metadata(output);identity,manifest=make_bundle(endpoint,output);negative=negatives(identity,manifest,endpoint)
    os.environ['XCP_P_IDENTITY']=str(output/'identity.bin')
    driver=EndpointDriver(endpoint,capture);bridge=PtyBridge(driver,capture);session=None
    try:
        serial=CapturingSerial(str(bridge.path),capture,EXTERNAL_19200_SXI)
        m=Master("pserialtransport",config=build_application(str(bridge.path),EXTERNAL_19200_SXI),transport_layer_interface=serial)
        m.transport.connect();session=m
        connect=m.connect();assert connect.maxCto==8 and connect.maxDto==8
        observed=read_bytes(m,0x10400,128)
        try:
            admit_identity(observed,bytes(128),manifest,endpoint,allow_host_fixture=True)
        except ValueError:
            pass
        else:
            raise AssertionError("stale host bundle did not refuse before writes")
        prewrite=[json.loads(line) for line in (output/'attribution.jsonl').read_text().splitlines()]
        assert not any(e.get('request_pdu_hex','').startswith(('e6','f9','f0')) for e in prewrite), "write or FREEZE preceded identity admission"
        admit_identity(observed,identity,manifest,endpoint,allow_host_fixture=True)
        pag=m.getPagProcessorInfo();assert pag.maxSegments==1 and pag.pagProperties.freezeSupported
        assert m.getSegmentInfo(0,0,0,0).basicInfo==0x1000
        assert m.getSegmentInfo(0,0,1,0).basicInfo==4
        assert m.getSegmentInfo(1,0,0,0).maxPages==1
        assert m.getPageInfo(0,0).init_segment==0
        assert m.getCalPage(1,0)==0 and m.getCalPage(2,0)==0
        assert m.setCalPage(3,0,0)==b''
        m.setMta(0x1000,0);assert m.download((2000).to_bytes(2,'little'))==b''
        assert m.getStatus().sessionStatus.storeCalRequest is False
        assert m.setSegmentMode(1,0)==b'' and m.getSegmentMode(0)==1
        assert m.setRequest(1,0)==b''
        assert m.getStatus().sessionStatus.storeCalRequest is True
        m.setMta(0x1000,0);assert m.download((3000).to_bytes(2,'little'))==b''
        pending=read_bytes(m,0x10000,640);assert pending[:8]==b'BLXPVW01' and pending[128:132]==(2000).to_bytes(2,'little')+(500).to_bytes(2,'little') and pending[384:388]==b'\xff'*4
        assert driver.request('drive 0')==['settled true']
        assert m.getStatus().sessionStatus.storeCalRequest is False
        durable=read_bytes(m,0x10000,640);assert durable[128:132]==pending[128:132]==durable[384:388]
        live=read_bytes(m,0x1000,2)+read_bytes(m,0x1002,2)
        assert live==(3000).to_bytes(2,'little')+(500).to_bytes(2,'little')
        assert struct.unpack_from('<I',durable,36)[0]==2 and struct.unpack_from('<Q',durable,80)[0]==1
        events=[json.loads(line) for line in (output/'attribution.jsonl').read_text().splitlines()]
        f9=[e for e in events if e.get('request_pdu_hex')=='f9010000'];assert len(f9)==1 and f9[0]['response_pdu_hex']=='ff'
        result={'status':'pass','scope':'host_fixture_only','client':{'pyxcp':md.version('pyxcp'),'pya2ldb':md.version('pya2ldb'),'modified':False,'General.disable_error_handling':True},'metadata':metadata,'negative_identity_controls':negative,'f9_requests':len(f9),'captured':durable[128:132].hex(),'durable':durable[384:388].hex(),'live_ram':live.hex(),'physical_uart':False,'mcu_image':False}
        capture.write_json('result.json',result);capture.write_json('environment.json',environment_metadata());return result
    finally:
        if session is not None:session.transport.close()
        bridge.close();stderr=driver.close()
        if stderr:capture.write_json('stderr.json',{'stderr':stderr})
        capture.finalize_manifest()

def main():
    parser=argparse.ArgumentParser();parser.add_argument('--endpoint',type=pathlib.Path,required=True);parser.add_argument('--output',type=pathlib.Path,required=True);args=parser.parse_args();print(json.dumps(run(args.endpoint.resolve(),args.output.resolve()),indent=2))
if __name__=='__main__':main()
