"""Normative constraints over explicit design witnesses, not a wire endpoint model."""
import copy,json,pathlib
ROOT=pathlib.Path(__file__).resolve().parents[2]
def check_trace(t):
 prev=None;accepted_at=None;responses=0
 for s in t['steps']:
  assert s['events']==[], 'event'
  assert s['status'] in (0,1) and s['freeze'] in (0,1), 'bit'
  if s['response'] is not None:
   assert s['response']=='ff' and s['phase']=='accepted','second_or_negative_response'
   responses+=1;assert responses==1,'second_or_negative_response'
  if s['accepted']:
   assert s['key']==[5,3,9] and s['capture_hex']=='b80bee02','identity_capture'
   if accepted_at is None:
    assert s['phase']=='accepted' and s['response']=='ff' and s['freeze']==1,'acceptance'
    accepted_at=s
   if s['connection']==3:
    assert s['status']==(0 if s['state']=='durable' else 1),'pending_disposition'
   else:
    assert s['connection']==4 and s['phase']=='diagnostic_reconnect','connection'
    assert s['status']==0 and s['freeze']==0,'new_connection_bits'
   if s['state']=='durable':
    assert s['durable_hex']==s['capture_hex'] and s['ownership_settled'] and s['backend_idle'],'durable_proof'
   else:assert s['durable_hex']=='d007fa00','old_durable_preserved'
  else:
   assert accepted_at is None and s['key'] is None and s['capture_hex'] is None and not s['custody'],'preadmission'
   assert s['status']==0 and s['response'] is None,'receipt_is_not_acceptance'
  if prev and prev['accepted']:
   assert s['accepted'] and s['key']==prev['key'] and s['capture_hex']==prev['capture_hex'],'retention'
   if not prev['ownership_settled'] and not s['ownership_settled']:assert s['custody'],'late_custody'
   if prev['custody'] and not s['custody']:
    assert s['ownership_settled'] and s['backend_idle'] and prev['recorded'],'release'
   if not prev['recorded'] and s['recorded']:assert s['ownership_settled'] and s['state'] in ('failed','durable'),'record'
   if s['connection']!=prev['connection']:
    assert s['phase']=='diagnostic_reconnect' and prev['wire_drained'] and prev['all_clients_drained'],'drain'
    assert prev['backend_idle'] and prev['ownership_settled'] and not prev['custody'] and prev['recorded'],'reconnect_custody'
    assert s['state']==prev['state']=='failed','reconnect_not_success'
  prev=s
 assert accepted_at is not None

def main():
 policy=json.loads((ROOT/'specs/profile-p-open-v2/policy.json').read_text());d=json.loads((ROOT/'specs/profile-p-design-v1/descriptor.json').read_text());v=json.loads((ROOT/'specs/profile-p-design-v1/vectors.json').read_text())
 assert policy['release_eligible'] is False and policy['standards_conformance_claim'] is False
 seg=policy['segment'];assert (seg['number'],seg['page'],seg['init_segment'],seg['count'],seg['page_count'])==(0,0,0,1,1)
 assert (seg['address'],seg['length'],seg['properties'])==(4096,4,63)
 assert all(seg[k]==d['calibration'][j] for k,j in [('number','segment'),('page','page'),('init_segment','init_segment'),('address','address'),('length','size')])
 assert seg['initialization']=='boot_only_self_reference_no_independent_image'
 async_p=policy['asynchronous'];assert async_p==dict(success_status='ff0000000000',unfulfilled_status='ff0100000000',event_packets=[],post_accept_negative_response=False,failure_clears_pending=False,reconnect_requires_idle_and_drain=True,new_connection_status=0,new_connection_freeze=0,diagnostic_reconnect_allows_save=False,uncertain_releases_custody=False)
 assert policy['storage_extra_bytes']==0 and d['storage']['total']==5120 and d['storage']['ceiling']==6144
 expected={'e9':'ff0101','e801000000':'ff0100000000','e7000000':'ff3f00','e800000000':'ff00000000100000','e800000100':'ff00000004000000'}
 for request,response in expected.items():assert any(x['request']==request and x['response']==response for x in v['commands'])
 traces=json.loads((ROOT/'specs/profile-p-open-v2/traces.json').read_text())['traces']
 for t in traces:check_trace(t)
 # Mutations target independently meaningful safety boundaries; expected assertion reason is checked.
 cases=[(0,0,'status',1,'receipt_is_not_acceptance'),(0,1,'response','ff','second_or_negative_response'),(0,2,'freeze',0,'acceptance'),(0,3,'capture_hex','a00f5802','identity_capture'),(0,3,'key',[5,3,10],'identity_capture'),(0,3,'durable_hex','b80bee02','old_durable_preserved'),(0,4,'ownership_settled',False,'durable_proof'),(0,4,'durable_hex','a00f5802','durable_proof'),(0,6,'backend_idle',False,'durable_proof'),(1,3,'status',0,'pending_disposition'),(1,3,'response','fe24','second_or_negative_response'),(1,3,'events',['fd03'],'event'),(1,3,'custody',False,'release'),(1,6,'wire_drained',False,'drain'),(1,6,'all_clients_drained',False,'drain'),(1,7,'status',1,'new_connection_bits'),(1,7,'freeze',1,'new_connection_bits'),(2,3,'custody',False,'late_custody'),(2,4,'status',0,'pending_disposition')]
 controls=[]
 for ti,si,key,value,reason in cases:
  t=copy.deepcopy(traces[ti]);t['steps'][si][key]=value
  try:check_trace(t)
  except AssertionError as e:assert str(e)==reason,(ti,si,key,str(e),reason)
  else:raise AssertionError(('mutation accepted',ti,si,key))
  controls.append(dict(trace=t['name'],step=si,field=key,rejected_at=reason))
 print(json.dumps(dict(scope='design-only',traces=len(traces),literal_PAG_pairs=len(expected),mutation_controls=controls),indent=2))
if __name__=='__main__':main()
