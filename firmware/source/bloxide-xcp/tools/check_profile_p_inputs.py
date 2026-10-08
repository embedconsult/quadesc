#!/usr/bin/env python3
"""Verify the exact local reviewed source and copied host metadata inputs."""
import hashlib,json,pathlib,subprocess,sys
root=pathlib.Path(__file__).resolve().parents[1]
lock=json.loads((root/'integration-lock.json').read_text())
pin=lock['profile_p_host']
persistence=root.parent/'inputs/bloxide-persistence'
calibration=root.parent/'inputs/bloxide-calibration'
framework=root.parent/'inputs/bloxide'
def git(path,*args):return subprocess.check_output(['git',*args],cwd=path,text=True).strip()
assert git(persistence,'rev-parse','HEAD')==pin['persistence_commit']
assert git(persistence,'rev-parse','HEAD^{tree}')==pin['persistence_tree']
assert git(calibration,'rev-parse','HEAD')==pin['calibration_commit']
for path in [persistence,calibration]:assert not git(path,'status','--porcelain=v1')
selected=lock['selected_composition']
assert git(framework,'rev-parse','HEAD')==selected['framework_commit']
assert git(framework,'rev-parse','HEAD^{tree}')==selected['framework_tree']
assert not git(framework,'status','--porcelain=v1')
files={'metadata.a2l':persistence/'specs/profile-p-design-v1/metadata.a2l','descriptor.json':persistence/'specs/profile-p-design-v1/descriptor.json','policy.json':persistence/'specs/profile-p-open-v2/policy.json'}
hashes={}
for name,source in files.items():
    copied=root/'interop/fixtures/profile-p-host'/name
    assert source.read_bytes()==copied.read_bytes(),name
    hashes[name]=hashlib.sha256(copied.read_bytes()).hexdigest()
assert lock['accepted_inputs']['t08_protocol']['commit']=='55ee09ae6820f43151644f079769dbb27fee7420'
assert lock['accepted_inputs']['t10_interop']['commit']=='8272bbec14f5336c4838b17b55ffbb547ee299de'
assert lock['framework']['commit']=='43c4faa14a934fc96b73c39978084220a273d5d0'
assert lock['contract_bundle']['archive_sha256']=='f6f75d443d96dd762d3640e1acc76480243eabc88fd441574b4cad4f4f6705db'
result={'status':'pass','persistence_commit':pin['persistence_commit'],'persistence_tree':pin['persistence_tree'],'calibration_commit':pin['calibration_commit'],'framework_commit':selected['framework_commit'],'copied_metadata_sha256':hashes,'source_mode':'selected local inputs','production_image':False}
if len(sys.argv)>1:pathlib.Path(sys.argv[1]).write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps(result,indent=2))
