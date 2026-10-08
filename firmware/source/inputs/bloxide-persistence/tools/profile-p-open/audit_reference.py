"""Independent literal/file audit; no dependency on reference probe functions."""
import hashlib,json,pathlib,struct,sys
hub=pathlib.Path(sys.argv[1]).resolve();results=[]
for mode,total in [('default',58),('single',49)]:
 p=hub/'artifacts'/('probe-'+mode);rows=json.loads((p/'exchanges.json').read_text());assert len(rows)==total
 by={r['label']:r for r in rows}
 for r in rows:
  for frame,field in [('tx_frame','request'),('rx_frame','response')]:
   b=bytes.fromhex(r[frame]);assert struct.unpack('<H',b[:2])[0]==len(b)-4;assert b[4:].hex()==r[field]
  assert bytes.fromhex(r['response'])[0] in (254,255), 'no unsolicited events in recorded exchanges'
 assert by['pag']['response']=='ff0301'
 assert by['segment-info']['response']=='ff0200000000'
 assert by['page0']['response']==by['other-page0']['response']=='ff3f01'
 assert by['page1']['response']==('ff0f01' if mode=='default' else 'fe26')
 initial=(p/'00-initial.bin').read_bytes();first=(p/'02-one-selected.bin').read_bytes();second=(p/'04-reenabled.bin').read_bytes()
 assert len(initial)==len(first)==len(second)==1064
 assert initial==(p/'01-no-selection.bin').read_bytes()
 assert first==(p/'03-cleared.bin').read_bytes()
 assert [i for i,(a,b) in enumerate(zip(initial,first)) if a!=b]==list(range(1056,1060))
 assert first[1056:1060]==bytes.fromhex('b80bee02') and first[800:804]==bytes.fromhex('e803f401')
 assert [i for i,(a,b) in enumerate(zip(first,second)) if a!=b]==list(range(800,804))
 assert second[800:804]==bytes.fromhex('a00f5802') and second[1060:1064]==bytes.fromhex('d007fa00')
 boot_reads={i:[r['response'] for r in rows if r['boot']==i and r['label']=='read-1'] for i in range(1,5)}
 assert boot_reads[2]==['ffe803f401'] and boot_reads[4]==['ffa00f5802']
 assert by['synchronous-open-failure']['response']=='fe24'
 a2l=next((p/'files').glob('*.a2l')).read_text()
 if mode=='single':
  assert '1 /* pages */' in a2l and '2 /* pages */' not in a2l
  assert 'XCP_READ_ACCESS_WITH_ECU_ONLY' in a2l and 'OPTIONAL_CMD SET_CAL_PAGE' not in a2l
  assert by['single-no-page-switch']['response']=='fe20'
 else:
  assert '2 /* pages */' in a2l and 'OPTIONAL_CMD SET_CAL_PAGE' in a2l
  assert by['ecu-page']['response'].endswith('01') and by['xcp-page']['response'].endswith('00')
  assert boot_reads[3]==['ffa00f5802','ff88136400']
  labels=[r['label'] for r in rows]
  assert labels.index('select-divergent')<labels.index('verify-divergent-selected')<labels.index('save-divergent')
  assert by['verify-divergent-selected']['response'].endswith('01')
  assert (p/'05-before-divergent.bin').read_bytes()==(p/'06-divergent.bin').read_bytes()
 results.append(dict(mode=mode,exchanges=len(rows),fresh_save_restores=False,loaded_save_restores=True,selected_divergence_tested=mode=='default'))
manifest=json.loads((hub/'artifacts/reference-source-manifest.json').read_text());src=next((hub/'research').glob('XCPlite-*'))
for row in manifest:assert hashlib.sha256((src/row['path']).read_bytes()).hexdigest()==row['sha256']
print(json.dumps(dict(campaigns=results,unmodified_reference_files=len(manifest),scope='host reference artifacts, not async P or hardware'),indent=2))
