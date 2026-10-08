#!/usr/bin/env python3
"""Finite algebra/raw-image checks of proposed r2, NOT a product service/driver.
No command queue, runtime, flash I/O, allocation freeze or hardware is implemented.
The arbitrary-subset theorem is in specs/erase-integrity-proof.md.
"""
import hashlib
import itertools
import json
from pathlib import Path
import random
import struct

ROOT = Path(__file__).resolve().parents[1]
MAX = (1 << 64) - 1
TABLE = []
for n in range(256):
    c = n
    for _ in range(8):
        c = (c >> 1) ^ (0x82F63B78 if c & 1 else 0)
    TABLE.append(c)


def crc(b):
    c = 0xFFFFFFFF
    for x in b:
        c = (c >> 8) ^ TABLE[(c ^ x) & 255]
    return c ^ 0xFFFFFFFF


def logical(seq=7, period=2000, duty=250, revision=10, fmt=2, schema=1,
            epoch=1, generation=1, operation=None):
    b = bytearray(b'\xff' * 320)
    b[:8] = b'BLXPERS2'
    struct.pack_into('<HHIQQIIQ', b, 8, fmt, schema, 4, seq, epoch, revision,
                     generation, seq if operation is None else operation)
    b[48:60] = bytes(12)
    struct.pack_into('<HH', b, 64, period, duty)
    checksum(b)
    return bytes(b)


def checksum(b):
    struct.pack_into('<I', b, 60, crc(b[:60] + b[64:320]))


def image(body, g=8, e=1024):
    m = (640 + g - 1) // g * g
    assert len(body) == 320 and m + g <= e
    b = bytearray(b'\xff' * e)
    b[:640:2] = body
    b[1:640:2] = bytes(x ^ 255 for x in body)
    b[m:m+g] = bytes(g)
    return bytes(b)


def classify(b, g=8, read_error=False):
    """Direct spec oracle for this design probe; never a future product import."""
    m = (640 + g - 1) // g * g
    if read_error:
        return 'Unreadable', None
    if b == b'\xff' * len(b):
        return 'Empty', None
    if b[m:m+g] != bytes(g):
        return 'Uncommitted', None
    if any(x ^ y != 255 for x, y in zip(b[:640:2], b[1:640:2])):
        return 'Corrupt', None
    l = bytes(b[:640:2])
    fmt, schema, n, seq = struct.unpack_from('<HHIQ', l, 8)
    if (l[:8] != b'BLXPERS2' or not schema or not seq or not 1 <= n <= 256
            or l[48:60] != bytes(12) or l[64+n:] != b'\xff' * (256-n)
            or b[640:m] != b'\xff' * (m-640)
            or b[m+g:] != b'\xff' * (len(b)-m-g)
            or struct.unpack_from('<I', l, 60)[0] != crc(l[:60] + l[64:])):
        return 'Corrupt', None
    if fmt != 2 or schema != 1:
        return 'Unsupported', l
    if n != 4:
        return 'Corrupt', None
    period, duty = struct.unpack_from('<HH', l, 64)
    if not 100 <= period <= 10000 or duty > 1000:
        return 'Corrupt', None
    return 'Valid', l


def recover(a, b, g=8):
    c = [classify(a, g), classify(b, g)]
    if any(kind == 'Unsupported' for kind, _ in c):
        return 'DefaultsUnsupported', None
    valid = [l for kind, l in c if kind == 'Valid']
    if not valid:
        return 'Defaults', None
    seq = lambda l: int.from_bytes(l[16:24], 'little')
    if len(valid) == 2 and seq(valid[0]) == seq(valid[1]) and valid[0] != valid[1]:
        return 'DefaultsAmbiguous', None
    return 'Record', max(valid, key=seq)


def eligible(b, selected, g=8, read_error=False, writable=True):
    """Q plus specified global gates; caller separately owns wear/quarantine."""
    if read_error or not writable:
        return False
    kind, l = classify(b, g)
    if kind == 'Unsupported':
        return False
    m = (640 + g - 1) // g * g
    if any(b[m:m+g]):
        return True  # Q1
    if any(x & y for x, y in zip(b[:640:2], b[1:640:2])):
        return True  # Q2: at least one 11 pair
    if (any(x ^ y != 255 for x, y in zip(b[:640:2], b[1:640:2]))
            or b[640:m] != b'\xff' * (m-640)
            or b[m+g:] != b'\xff' * (len(b)-m-g)):
        return False
    if kind == 'Corrupt':
        return True  # Q3: complementary but invalid logical envelope/semantics
    if kind == 'Valid' and selected is not None:
        return int.from_bytes(l[16:24], 'little') < int.from_bytes(selected[16:24], 'little') or l == selected
    return False


def erase_reachable(a, b):
    return all(x | y == y for x, y in zip(a, b))


def program_reachable(a, b):
    return all(x & y == y for x, y in zip(a, b))


def pair_checks():
    # Exhaust EVERY two-bit pre/post-state under each monotone order.
    rows = []
    for x, y in itertools.product(range(4), repeat=2):
        er, pr = x | y == y, x & y == y
        if x in (1, 2) and y in (1, 2) and (er or pr):
            assert x == y
        if x == 3 and er:
            assert y == 3
        rows.append(dict(before=x, after=y, erase=er, program=pr))
    # Exhaust 256 byte codewords x256 erase masks on EACH rail (131072 trials).
    n = 0
    for x, mask, rail in itertools.product(range(256), range(256), range(2)):
        a, b = (x | mask, x ^ 255) if rail == 0 else (x, (x ^ 255) | mask)
        assert a ^ b != 255 or (a == x and b == x ^ 255)
        n += 1
    return dict(two_bit_transition_table=rows, byte_rail_masks=n)


def directed_searches():
    groups = {
        'sequence': [logical(seq=x) for x in [1, 2, 6, 7, 8, 14, MAX-1, MAX]],
        'format': [logical(fmt=x) for x in [0, 1, 2, 3, 6, 65535]],
        'schema': [logical(schema=x) for x in [1, 2, 3, 7, 65535]],
        'duplicate_sequence': [logical(seq=7, period=p, duty=d, revision=v)
                               for p, d, v in [(1000, 500, 9), (2000, 250, 10),
                                               (3000, 750, 11), (2000, 250, 11)]],
        'duplicate_sequence_creation': [logical(seq=6, period=1000, duty=500, revision=9),
                                        logical(seq=7, period=1000, duty=500, revision=9, operation=6)],
        'source_labels_and_request': [logical(epoch=e, revision=v, generation=g, operation=o)
                                      for e, v, g, o in [(1, 10, 1, 7), (2744301397, 10, 1, 7),
                                                         (1, 11, 1, 7), (1, 10, 3, 7), (1, 10, 1, 14)]]}
    report = {}
    for name, bodies in groups.items():
        words = [image(l) for l in bodies]
        assert all(classify(w)[0] in ('Valid', 'Unsupported') for w in words)
        tested = 0
        for i, j in itertools.permutations(range(len(words)), 2):
            assert words[i] != words[j]
            assert not erase_reachable(words[i], words[j]), (name, i, j)
            assert not program_reachable(words[i], words[j]), (name, i, j)
            tested += 1
        report[name] = dict(words=len(words), directed_distinct_pairs=tested,
                            erase_transformations=0, program_transformations=0)
    # Every logical bit, including CRC, reserved and unused payload, is covered.
    a = logical()
    for bit in range(320*8):
        b = bytearray(a)
        b[bit//8] ^= 1 << (bit % 8)
        assert not erase_reachable(image(a), image(b))
        assert not program_reachable(image(a), image(b))
    report['all_logical_field_bits_including_stored_crc'] = 2560
    return report


def witness():
    p = ROOT/'tests/original-f1/logs'
    old, torn = [(p/name).read_bytes() for name in ['inactive-b-before.bin', 'inactive-b-torn-erase.bin']]
    mask = (p/'erase-mask.bin').read_bytes()
    assert bytes(x ^ y for x, y in zip(old, torn)) == mask
    assert erase_reachable(old, torn)
    assert sum(x.bit_count() for x in mask) == 18
    assert old[320:328] == torn[320:328] == bytes(8)
    a = image(logical())
    b = image(logical(seq=6, period=1000, duty=500, revision=9))
    changed = bytearray(b)
    for i, delta in enumerate(mask[:320]):
        changed[2*i] |= delta  # exact logical-offset masks on data rails; complement unchanged
    assert erase_reachable(b, changed) and changed[640:648] == b[640:648]
    assert classify(changed)[0] == 'Corrupt'
    assert recover(a, changed) == ('Record', logical())
    # Attempt a FULLY valid corrected equivalent, with a freshly correct stored CRC.
    desired = image(logical(seq=14, period=1000, duty=500, revision=9,
                            epoch=2744301397, operation=6))
    assert classify(desired)[0] == 'Valid'
    assert not erase_reachable(b, desired)
    out = ROOT/'evidence/r2-vectors'
    out.mkdir(exist_ok=True)
    files = {}
    for name, data in [('active-a.bin', a), ('inactive-b.bin', b), ('witness-data-rails.bin', changed),
                       ('valid-promotion-target.bin', desired),
                       ('intended-new.bin', image(logical(seq=8, period=3000, duty=750, revision=11)))]:
        (out/name).write_bytes(data)
        files[name] = hashlib.sha256(data).hexdigest()
    return dict(original_changed_bits=18, original_crc_before=old[60:64].hex(),
                original_crc_after=torn[60:64].hex(), r2_class='Corrupt',
                r2_selected_sequence=7, fully_valid_promotion_erase_reachable=False,
                vector_sha256=files)


def prestate_and_edges():
    selected = logical()
    a = image(selected)
    high = image(logical(seq=14, period=1000, duty=500, revision=9))
    # A single 00 pair: structurally invalid, but erase repairs it into a newer record.
    zero = bytearray(high); zero[0] &= ~2  # data bit1 was one; complement bit1 already zero
    assert classify(zero)[0] == 'Corrupt' and not eligible(zero, selected)
    assert erase_reachable(zero, high) and recover(a, high)[1] != selected
    tail = bytearray(high); tail[-1] &= 254
    assert classify(tail)[0] == 'Corrupt' and not eligible(tail, selected)
    assert erase_reachable(tail, high)
    # G256 has physical alignment padding too; corrupt padding can be repaired.
    padded = bytearray(image(logical(seq=14), 256)); padded[640] = 254
    assert not eligible(padded, selected, 256)
    assert erase_reachable(padded, image(logical(seq=14), 256))
    one = bytearray(high); one[0] |= 1  # creates11 pair
    marker = bytearray(zero); marker[640] = 1
    badcrc = bytearray(selected); badcrc[60] ^= 1
    badsemantic = logical(period=99)
    safe = [one, marker, image(badcrc), image(badsemantic), b'\xff'*1024]
    rng = random.Random(1701)
    count = 0
    for b in safe:
        assert eligible(b, selected)
        for _ in range(256):
            changed = bytes(x | rng.getrandbits(8) for x in b)
            assert classify(changed)[0] not in ('Valid', 'Unsupported')
            assert eligible(changed, selected)
            assert recover(a, changed) == ('Record', selected)
            count += 1
    assert not eligible(high, selected)  # recovery discrepancy, not inactive
    assert not eligible(a, None)
    assert not eligible(a, selected, read_error=True)
    assert not eligible(a, selected, writable=False)
    unknown = image(logical(fmt=3))
    assert not eligible(unknown, selected)
    assert recover(a, unknown)[0] == 'DefaultsUnsupported'
    assert classify(b'\xff'*1024, read_error=True)[0] == 'Unreadable'
    assert recover(a, a) == ('Record', selected)
    assert recover(a, image(logical(period=1000)))[0] == 'DefaultsAmbiguous'
    assert classify(image(logical(seq=0)))[0] == 'Corrupt'
    assert classify(image(logical(schema=0)))[0] == 'Corrupt'
    # No implicit v1 support: retain raw v1 bytes, pad to current geometry only.
    raw = (ROOT/'tests/original-f1/logs/active-a.bin').read_bytes()+b'\xff'*512
    assert classify(raw)[0] not in ('Valid', 'Unsupported')
    # Explicit arithmetic exhaustion, not wrapping.
    next_sequence = lambda seq: None if seq == MAX else seq + 1
    assert next_sequence(MAX) is None and next_sequence(MAX-1) == MAX
    for seq in (1, 2, 6, 7, 8, 14, MAX-1, MAX):
        b = image(logical(seq=seq))
        assert recover(b, b)[1] == logical(seq=seq)
    # Same marker value across different records does not permit a body substitution.
    b = image(logical(seq=8, period=3000, duty=750, revision=11))
    assert a[640:648] == b[640:648] == bytes(8)
    assert not erase_reachable(a, b)
    for n in range(1, 256):
        t = bytearray(b); t[640] = n
        assert classify(t)[0] == 'Uncommitted' and not erase_reachable(t, b)
    return dict(Q_unsafe_repair_negative_controls=3, Q_safe_subset_trials=count,
                nonzero_marker_cannot_erase_to_zero=255, edge_controls='passed')


def cut_checks():
    geometries = [(1,1024), (8,1024), (16,1024), (64,4096), (256,4096)]
    rows = []
    for g, e in geometries:
        m = (640+g-1)//g*g
        old, prev, new = [image(x, g, e) for x in
                         [logical(), logical(seq=6,period=1000,duty=500,revision=9),
                          logical(seq=8,period=3000,duty=750,revision=11)]]
        counts = dict(prefix=0, subset=0, g1_masks=0)
        for blank, swap in [(True,True), (False,False), (False,True)]:
            a = b'\xff'*e if blank else old
            p = b'\xff'*e if blank else prev
            selected = None if blank else logical()
            candidate = image(
                logical(seq=1, period=3000, duty=750, revision=11, operation=8),
                g,
                e,
            ) if blank else new
            candidate_body = candidate[:640:2]
            if blank:
                # Independent allocation assertion: do not derive the expected
                # first sequence from a pre-existing seq8 fixture. `swap=True`
                # places the candidate in slot A.
                assert int.from_bytes(candidate_body[16:24], 'little') == 1
                assert swap
            expected = {('Defaults',None), ('Record',candidate_body)} if blank else {('Record',selected), ('Record',candidate_body)}
            assert eligible(p, selected, g)
            def check(t):
                result = recover(t,a,g) if swap else recover(a,t,g)
                assert result in expected, (g,e,blank,swap,result)
                # A cut leaves Q-eligible inactive media for a future authorized retry.
                # If new became selected, untouched old is now the target instead.
                qtarget = a if result == ('Record',candidate_body) else t
                assert eligible(qtarget, result[1], g)
            for k in range(e+1):
                check(b'\xff'*k+p[k:]); counts['prefix'] += 1
            for off in range(0,m+g,g):
                for k in range(g+1):
                    check(candidate[:off+k]+b'\xff'*(e-off-k)); counts['prefix'] += 1
        for seed in (17,1701):
            rng = random.Random(seed)
            for i in range(2048):
                if i%3 == 0:
                    t = bytes(x | rng.getrandbits(8) for x in prev)
                else:
                    off = rng.randrange(m//g)*g if i%3 == 1 else m
                    t = new[:off]+bytes(255 ^ ((255 ^ x) & rng.getrandbits(8)) for x in new[off:off+g])+b'\xff'*(e-off-g)
                assert recover(old,t,g) in {('Record',logical()), ('Record',new[:640:2])}
                counts['subset'] += 1
        if g == 1:
            for off in range(m+1):
                for mask in range(256):
                    t = new[:off]+bytes([255 ^ ((255 ^ new[off]) & mask)])+b'\xff'*(e-off-1)
                    assert recover(old,t,g) in {('Record',logical()), ('Record',new[:640:2])}
                    counts['g1_masks'] += 1
        rows.append(dict(G=g,E=e,counts=counts))
    return rows


def repeated_boundaries():
    # Two admitted saves, first cut at every command boundary; recovered media feeds
    # the second save. Backend requalification/wear are preconditions, not simulated.
    m, g, e = 640, 8, 1024
    original = [image(logical()), image(logical(seq=6,period=1000,duty=500,revision=9))]
    first = logical(seq=8,period=3000,duty=750,revision=11)
    # 83 states: pre-erase, aftererase, aftereach80body, aftercommit.
    def states(pre, body):
        desired = image(body)
        yield pre
        for off in range(0,m+g+1,g):
            yield desired[:off]+b'\xff'*(e-off)
    count = 0
    for b in states(original[1],first):
        slots = [original[0],b]
        selected = recover(*slots)[1]
        idx = 0 if selected == first else 1
        assert eligible(slots[idx],selected)
        nseq = int.from_bytes(selected[16:24],'little')+1
        second = logical(seq=nseq,period=4000,duty=500,revision=12)
        for t in states(slots[idx],second):
            trial = slots.copy(); trial[idx] = t
            assert recover(*trial) in {('Record',selected),('Record',second)}
            count += 1
    assert count == 83**2
    return count


def single_bits():
    a, b = image(logical()), image(logical(seq=6,period=1000,duty=500,revision=9))
    n = 0
    for slot, word in enumerate((a,b)):
        for bit in range(len(word)*8):
            t = bytearray(word); t[bit//8] ^= 1 << (bit%8)
            assert classify(t)[0] not in ('Valid','Unsupported')
            result = recover(t,b) if slot == 0 else recover(a,t)
            assert result == ('Record', b[:640:2] if slot == 0 else a[:640:2])
            n += 1
    return n


def main():
    assert crc(b'123456789') == 0xE3069283
    result = dict(scope='finite design algebra and raw-image checks only; no product or physical proof',
                  pair_checks=pair_checks(), directed=directed_searches(), witness=witness(),
                  prestate=prestate_and_edges(), cuts=cut_checks(),
                  repeated_boundary_pairs=repeated_boundaries(), single_bit_corruptions=single_bits(),
                  status='passed', product_tests_executed=0, hardware_tests_executed=0,
                  theorem='specs/erase-integrity-proof.md conditional on unqualified R1/T16 model')
    print(json.dumps(result,indent=2))


if __name__ == '__main__':
    main()
