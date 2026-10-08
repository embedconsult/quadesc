#!/usr/bin/env python3
# Copyright 2025 Bloxide, all rights reserved
"""Materialize independent source checkouts; invoke only cargo-blox generation.
Build logs and evidence stay in the caller's job-local target directory.
"""
import argparse, json, os, pathlib, shutil, subprocess, hashlib
parser = argparse.ArgumentParser()
parser.add_argument('--update-locks', action='store_true')
args = parser.parse_args()
root = pathlib.Path(__file__).resolve().parents[1]
fixture = root / 'crates/tools/cargo-blox/tests/fixtures/embedded'
output = root / 'target/embedded-consumer'
output.mkdir(parents=True, exist_ok=True)
env = dict(os.environ, CARGO_BUILD_JOBS='4', RUSTUP_TOOLCHAIN='1.94.0',
           CARGO_TARGET_DIR=str(root / 'target/t06-fixture-build'))
commands = []
def run(command, cwd, label, expected=0):
    with (output / (label + '.log')).open('w') as log:
        result = subprocess.run([str(x) for x in command], cwd=cwd, env=env, stdout=log, stderr=subprocess.STDOUT)
    commands.append(dict(command=[str(x) for x in command], cwd=str(cwd), exit_code=result.returncode, log=label+'.log'))
    (output/'commands.json').write_text(json.dumps(commands, indent=2)+'\n')
    if (result.returncode == 0) != (expected == 0):
        raise SystemExit(f'{label}: exit {result.returncode}; see {output}/{label}.log')
    print(f'{label}: exit {result.returncode}', flush=True)
# This target subtree is entirely this fixture runner's disposable output.
for source in ['package', 'consumer']:
    destination = output / source
    if destination.exists(): shutil.rmtree(destination)
    for path in (fixture/source).rglob('*'):
        if not path.is_file(): continue
        relative = path.relative_to(fixture/source)
        if path.name.endswith('.in'): relative = relative.with_name(path.name[:-3])
        target = destination/relative
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(path.read_text().replace('@FRAMEWORK@', str(root)))
    (destination/'.gitignore').write_text('target/\n.vscode/\n')
    # Independent fixture source Git identities, no remotes or publication.
    run(['git','init','-q'], destination, source+'-git-init')
    run(['git','add','.'], destination, source+'-git-add')
    run(['git','-c','user.name=Fixture','-c','user.email=fixture@invalid','commit','-qm','Fixture source inputs'], destination, source+'-git-commit')
cli = root / 'target/t06-build/debug/cargo-blox'
consumer = output/'consumer'
run([cli,'blox','resolve'], consumer, 'resolve')
run([cli,'blox','generate'], consumer, 'generate')
if args.update_locks:
    run([cli,'blox','lock'], consumer, 'update-generated-lock')
    saved = fixture/'consumer/locks/blox-generated.Cargo.lock.in'
    saved.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(consumer/'locks/blox-generated.Cargo.lock', saved)
    run(['cargo','generate-lockfile'], consumer, 'update-source-lock')
    shutil.copyfile(consumer/'Cargo.lock', fixture/'consumer/Cargo.lock.in')
    # Lock updates changed framework fixture source content.
    run([cli,'blox','resolve'], consumer, 'resolve-after-lock-update')
else:
    assert (consumer/'locks/blox-generated.Cargo.lock').is_file(), 'run --update-locks once'
run([cli,'blox','generate','--locked'], consumer, 'locked-generate')
run([cli,'blox','test','--example','fixture-host','--','--locked'], consumer, 'host-actions')
run([cli,'blox','build','--example','fixture-host','--','--locked'], consumer, 'host-build')
for stage in ['init', 'service']:
    env['FIXTURE_FAIL'] = stage
    run([pathlib.Path(env['CARGO_TARGET_DIR'])/'debug/fixture-host'], consumer, 'startup-failure-'+stage, expected=1)
    assert 'platform startup failed before freeze and output' in (output/('startup-failure-'+stage+'.log')).read_text()
env.pop('FIXTURE_FAIL')
run([cli,'blox','build','--example','fixture-embedded','--','--locked','--release'], consumer, 'thumb-link')
run(['cargo','tree','--manifest-path',consumer/'target/bloxide-generated/Cargo.toml','-p','fixture-embedded','--target','thumbv8m.main-none-eabi','--locked','-e','features'], consumer, 'target-features')
features = (output/'target-features.log').read_text()
for forbidden in ['embassy-executor feature "arch-std"', 'embassy-time feature "std"', 'bloxide-core feature "std"']:
    assert forbidden not in features, forbidden

elf = pathlib.Path(env['CARGO_TARGET_DIR'])/'thumbv8m.main-none-eabi/release/fixture-embedded'
(output/'elf.json').write_text(json.dumps(dict(path=str(elf), sha256=hashlib.sha256(elf.read_bytes()).hexdigest(), size=elf.stat().st_size), indent=2)+'\n')
run(['readelf','-h','-S','-s',elf], consumer, 'elf-inspection')
# Prove sources/locks survive loss of disposable generation.
shutil.rmtree(consumer/'target/bloxide-generated')
run([cli,'blox','generate','--locked'], consumer, 'clean-locked-regenerate')
print(output)
