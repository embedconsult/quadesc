#!/usr/bin/env python3
"""Generate firmware map/defaults, A2L and board UI profile from one source contract."""
import json,pathlib,struct,hashlib
D=pathlib.Path(__file__).resolve().parent;c=json.loads((D/'contract.json').read_text());p=c['parameters']
assert len({x['id'] for x in p})==len(p) and len({x['address'] for x in p})==len(p)
s='// Generated from contract.json by generate.py; do not edit.\n'
s+=f'pub const COUNT:usize={len(p)};\npub const FIELDS:[Field;COUNT]=[\n'
for v in p:
 floating=v['type']=='FLOAT32_IEEE';initial=struct.unpack('<I',struct.pack('<f',v['default']))[0] if floating else v['default']
 s+=f"Field {{ id:{v['id']}, width:{v['width']}, min:{float(v['minimum'])}, max:{float(v['maximum'])}, initial:{initial}, writable:{str(not v['read_only']).lower()}, float:{str(floating).lower()} }},\n"
s+='];\npub const REGIONS:[xcp_core::Region;COUNT]=[\n'
for v in p:s+=f"xcp_core::Region::{'read_only' if v['read_only'] else 'calibration'}({v['id']}, {v['address']}, {v['width']}),\n"
s+='];\n'
for i,v in enumerate(p):s+=f"pub const {v['name'].upper()}:usize={i};\n"
lookup={v['name']:i for i,v in enumerate(p)};adc=c['board']['adc']
s+='pub const ADC_FIELDS:[[usize;4];30]=[\n'+''.join('['+','.join(str(lookup[v[k]]) for k in ['engineering','gain','offset','flags'])+'],\n' for v in adc)+'];\n'
s+='pub const DEFAULT_CAL:[[f32;2];30]=[\n'+''.join(f"[{float(v['default_gain'])},{float(v['default_offset'])}],\n" for v in adc)+'];\n'
s+='pub const ADC_KIND:[u8;30]=['+','.join(str({'current':1,'ntc_resistance':2,'spare':3}.get(v['kind'],0)) for v in adc)+'];\n'
s+='pub const ADC_MOTOR:[u8;30]=['+','.join(str(v.get('motor') or 0) for v in adc)+'];\n'
s+='pub const ADC_ASSUMPTIONS:[u32;30]=['+','.join(str(v['assumption_flags']) for v in adc)+'];\n'
(D/'src/generated.rs').write_text(s)
a='''ASAP2_VERSION 1 71
/begin PROJECT MAINBOARD "Schematic semantic characterization"
/begin MODULE SOM "Contract v7; firmware engineering values; see per-channel assumptions"
'''+(D/'XCP_104.aml').read_text()+'''
/begin MOD_COMMON "Little endian"
BYTE_ORDER MSB_LAST
ALIGNMENT_BYTE 1
ALIGNMENT_WORD 1
ALIGNMENT_LONG 1
ALIGNMENT_FLOAT32_IEEE 1
/end MOD_COMMON
'''
for ty in ['UWORD','ULONG','FLOAT32_IEEE']:a+=f'/begin RECORD_LAYOUT RL_{ty}\nFNC_VALUES 1 {ty} ROW_DIR DIRECT\n/end RECORD_LAYOUT\n'
for v in p:
 f=v['factor'];desc=v['description'].replace('"',"'")
 a+=f'''/begin COMPU_METHOD CM_{v['name']} "wire to physical" LINEAR "%16.7" "{v['unit']}"
COEFFS_LINEAR {f} 0
/end COMPU_METHOD
/begin CHARACTERISTIC {v['name']} "{desc}" VALUE 0x{v['address']:x} RL_{v['type']} 0 CM_{v['name']} {v['minimum']*f} {v['maximum']*f}
ECU_ADDRESS_EXTENSION 0
BYTE_ORDER MSB_LAST
PHYS_UNIT "{v['unit']}"
STEP_SIZE {0.000001 if v['type']=='FLOAT32_IEEE' else f}
'''+('READ_ONLY\n' if v['read_only'] else '')
 if v['name'] in ['pwm_action','drv_action','reboot_action','adc_diag_action','bench_action']:a+='/begin ANNOTATION\nANNOTATION_LABEL "XCP_UI_COMMAND"\n/begin ANNOTATION_TEXT\n"execute-last; no-save; self-clearing"\n/end ANNOTATION_TEXT\n/end ANNOTATION\n'
 if v.get('persist'):a+='/begin ANNOTATION\nANNOTATION_LABEL "CALIBRATION_PERSIST"\n/begin ANNOTATION_TEXT\n"RAM edit; explicit Save atomically stores whole LED and ADC calibration"\n/end ANNOTATION_TEXT\n/end ANNOTATION\n'
 a+='/end CHARACTERISTIC\n'
a+='''/begin IF_DATA XCP
/begin PROTOCOL_LAYER
0x0100
5000 5000 5000 5000 5000 5000 5000
8 8 BYTE_ORDER_MSB_LAST ADDRESS_GRANULARITY_BYTE
OPTIONAL_CMD SET_REQUEST
OPTIONAL_CMD SET_MTA
OPTIONAL_CMD UPLOAD
OPTIONAL_CMD DOWNLOAD
/end PROTOCOL_LAYER
/begin XCP_ON_CAN
0x0100
CAN_ID_MASTER 0x700
CAN_ID_SLAVE 0x701
BAUDRATE 500000
/end XCP_ON_CAN
/end IF_DATA
/end MODULE
/end PROJECT
'''
(D/'firmware.a2l').write_text(a)
profile=dict(c['board'],schema_version=c['version'],a2l_sha256=hashlib.sha256(a.encode()).hexdigest(),persist=[v['name'] for v in p if v.get('persist')])
(D/'board-profile.json').write_text(json.dumps(profile,indent=2)+'\n')
print('Generated',len(p),'typed scalars and matched board profile')
