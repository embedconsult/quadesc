"""F1/F2 design regression checks tied to unchanged source; never a service emulator."""
import copy
import hashlib
import json
import pathlib

class InvalidTrace(AssertionError):
    pass

def require(value, stage):
    if not value:
        raise InvalidTrace(stage)

PREFIX = ['Broker::reserve', 'capture_owner', 'Persistence::save -> Accepted',
          'summary <- K/C', 'F9 -> FF', 'take_command -> Read']
SETTLED = ['ownership_settled -> true', 'summary <- terminal/K/C',
           'Broker::record_terminal', 'Persistence::release', 'Broker::release']
UNSETTLED = ['ownership_settled -> false', 'retain command/control/K/C']
CASES = {
    'q-changed': (['complete -> Read(classification changed)'], 'Failed(Qualification,RecoveryChanged,true)', 9, True),
    'q-unsafe': (['complete -> Read(unchanged,Q unsafe)'], 'Failed(Qualification,UnsafeErasePrestate,true)', 9, True),
    'q-ecc': (['complete -> Read(UncorrectableEcc)'], 'Failed(Qualification,Read(UncorrectableEcc),true)', 7, True),
    'wear-denied': (['complete -> Read(full,Q safe)', 'take_command -> ConsumeErasePermit', 'complete -> WearDenied'], 'Failed(Wear,WearUnavailable,true)', 3, True),
    'q-backend-idle': (['complete -> Failed(quiescent=true)'], 'Failed(Qualification,Backend,true)', 1, True),
    'q-backend-unsettled': (['complete -> Failed(quiescent=false)'], 'Indeterminate(Qualification,Backend)', 1, False),
    'wear-backend-unsettled': (['complete -> Read(full,Q safe)', 'take_command -> ConsumeErasePermit', 'complete -> Failed(quiescent=false)'], 'Indeterminate(Wear,Backend)', 1, False),
}

def check_trace(t):
    if t['id'] == 'known-write-lock':
        require(t['events'] == ['health().write_locked == true', 'F9 -> FE24'], 'known-input-before-reserve')
        require(t['key'] is None and t['capture'] is None, 'no-new-key-capture')
        require(t['responses'] == ['fe24'] and t['outcome'] == 'RejectedBeforeReserve', 'known-denial-response')
        return
    events, outcome, reason, settled = CASES[t['id']]
    require(t['events'] == PREFIX + events + (SETTLED if settled else UNSETTLED), 'api-order-and-settlement')
    require(t['responses'] == ['ff'], 'single-accepted-response')
    require(t['terminal_key'] == t['key'] == [7, 3, 9], 'exact-retained-key')
    require(t['terminal_capture'] == t['capture'] == 'b80bee02', 'exact-retained-capture')
    require((t['outcome'], t['reason'], t['settled']) == (outcome, reason, settled), 'actual-service-outcome')

def check_rejection(before, after, t):
    # The controls hold other sampled observations constant. Full equality also
    # detects corruption of valid-but-wrong reason/key/C, unlike decode alone.
    require(t['accepted'] is False and t['commands'] == [], 'rejection-has-no-work')
    require(t['response'] == {'durable-rejected-next-save':'fe24', 'failed-rejected-next-save':'fe27'}[t['id']], 'rejection-wire')
    require(before == after, 'retained-operation-preservation')

def check_all(root):
    root = pathlib.Path(root)
    d = json.loads((root/'specs/profile-p-design-v1/admission-correction.json').read_text())
    require(d['scope'] == 'candidate_design_only_not_service_execution', 'scope')
    for name, digest in d['source_sha256'].items():
        require(hashlib.sha256((root/name).read_bytes()).hexdigest() == digest, 'source-pin')
    s = (root/'src/service.rs').read_text()
    save = s[s.index('    pub fn save'):s.index('    fn retain_rejected')]
    require(save.index('self.state = ServiceState::QualifyErase;') < save.index('SaveResponse::Accepted'), 'source-accept-before-q')
    require('CommandKind::' not in save and 'CompletionStatus::WearDenied' not in save, 'source-no-preflight')
    for literal in ['pub const fn health', 'pub write_locked: bool', 'CommandKind::ConsumeErasePermit',
                    'CompletionStatus::WearDenied =>', 'FailureKind::WearUnavailable',
                    'FailureKind::RecoveryChanged', 'FailureKind::UnsafeErasePrestate',
                    'CompletionStatus::Failed { quiescent: false }', 'pub const fn ownership_settled']:
        require(literal in s, 'source-api-tie')
    traces = {t['id']: t for t in d['api_traces']}
    require(set(traces) == set(CASES) | {'known-write-lock'} and len(traces) == len(d['api_traces']), 'trace-coverage')
    for t in traces.values():
        check_trace(t)
    c = json.loads((root/'specs/profile-p-design-v1/vectors.json').read_text())['commands']
    for t in c:
        if t['id'] in ('known-write-lock', 'known-q-lock', 'quarantine'):
            require(t['state'] == 'quarantine' and t['public_input'] == 'Persistence::health().write_locked == true', 'wire-known-input')
    negatives = []
    def reject(label, stage, fn):
        try:
            fn()
        except InvalidTrace as e:
            require(str(e) == stage, 'wrong-negative-stage')
            negatives.append(dict(control=label, expected_stage=stage, rejected=True))
        else:
            raise AssertionError('bad correction vector accepted: '+label)
    for label, field, value, stage in [
        ('post-C-FE24', 'responses', ['fe24'], 'single-accepted-response'),
        ('second-F9-response', 'responses', ['ff','fe24'], 'single-accepted-response'),
        ('wrong-terminal-key', 'terminal_key', [7,3,10], 'exact-retained-key'),
        ('recaptured-RAM', 'terminal_capture', 'a00ffa00', 'exact-retained-capture'),
        ('wear-as-rejection', 'outcome', 'RejectedBeforeReserve', 'actual-service-outcome')]:
        bad = copy.deepcopy(traces['wear-denied']);bad[field] = value
        reject(label, stage, lambda: check_trace(bad))
    bad = copy.deepcopy(traces['wear-denied']);bad['events'].insert(0, bad['events'].pop(7))
    reject('permit-before-C', 'api-order-and-settlement', lambda: check_trace(bad))
    bad = copy.deepcopy(traces['q-backend-unsettled']);bad['events'] += SETTLED
    reject('release-unsettled', 'api-order-and-settlement', lambda: check_trace(bad))
    bad = copy.deepcopy(traces['known-write-lock']);bad['events'][0] = 'assume next wear permit unavailable'
    reject('invented-preflight', 'known-input-before-reserve', lambda: check_trace(bad))
    ev = root/'evidence/profile-p-design-v1'
    require({t['id'] for t in d['rejected_next_save']} == {'durable-rejected-next-save', 'failed-rejected-next-save'}, 'retention-coverage')
    for t in d['rejected_next_save']:
        before = (ev/t['before']).read_bytes();after = (ev/t['after']).read_bytes()
        check_rejection(before, after, t)
        for label, offset in [('reason',52), ('state',36), ('key',56), ('capture',128)]:
            bad = bytearray(after);bad[offset] ^= 1
            reject(t['id']+'-mutated-'+label, 'retained-operation-preservation', lambda: check_rejection(before, bad, t))
    return dict(api_traces=len(traces), retained_rejection_vectors=2, negative_controls=negatives,
                scope='source-tied design checks only; no Rust/endpoint execution')

if __name__ == '__main__':
    print(json.dumps(check_all(pathlib.Path(__file__).resolve().parents[2]), indent=2))
