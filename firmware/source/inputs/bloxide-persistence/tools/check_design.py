#!/usr/bin/env python3
"""Check layout and enumerate planned cut locations. No media or product model."""
import copy
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def check_layout(layout):
    assert layout['endian'] == 'little'
    end = 0
    for field in layout['fields']:
        assert field['offset'] == end, ('gap/overlap', field)
        assert field['bytes'] > 0
        end += field['bytes']
    assert end == layout['header_bytes'] == 64
    assert layout['fields'][-1] == {'name': 'crc32c', 'offset': 60, 'bytes': 4}
    assert layout['max_payload_bytes'] == 256
    assert layout['granules'] == [1, 2, 4, 8, 16, 32, 64, 128, 256]
    assert layout['crc_domain'] == [[0, 60], [64, 320]]
    assert layout['crc_domain_space'] == 'decoded_logical'
    assert layout['logical_body_bytes'] == 320 and layout['encoded_body_bytes'] == 640
    assert layout['supported_record_format'] == 2
    assert layout['magic_hex'] == '424c585045525332'
    assert layout['field_offset_space'] == 'logical'
    assert layout['encoding'] == 'S[2*i]=L[i]; S[2*i+1]=L[i] XOR 255; i=0..319'
    assert layout['max_programmed_span'] == 1024
    assert layout['commit_offset'] == 'round_up(640,G)'
    assert layout['commit_bytes'] == 'G'
    assert layout['marker_byte'] == 0 and layout['padding_byte'] == 255
    for granule in layout['granules']:
        marker = ((640 + granule - 1) // granule) * granule
        assert marker % granule == 0
        assert 640 <= marker <= 768
        assert marker + granule <= 1024
        assert all(marker >= 2*(64 + length) for length in [1, 4, 8, 255, 256])


def prefixes(granule, erase):
    """Labels only: no bytes are changed and no recovery code is evaluated."""
    marker = ((640 + granule - 1) // granule) * granule
    for count in range(erase + 1):
        yield ('erase', 0, count)
    for offset in range(0, marker, granule):
        for count in range(granule + 1):
            yield ('body', offset, count)
    for count in range(granule + 1):
        yield ('commit', marker, count)


def main():
    layout = json.loads((ROOT / 'specs/layout.json').read_text())
    catalogue = json.loads((ROOT / 'tests/test-catalogue.json').read_text())
    check_layout(layout)
    negative = 0
    for index in range(len(layout['fields'])):
        bad = copy.deepcopy(layout)
        bad['fields'][index]['offset'] += 1
        try:
            check_layout(bad)
        except AssertionError:
            negative += 1
        else:
            raise AssertionError('layout gap/overlap not detected')
    ids = [row['id'] for row in catalogue['cases']]
    assert len(ids) == len(set(ids)) == 23
    acceptance = {a['id'] for a in catalogue['acceptance']}
    assert acceptance == {'A1', 'A2', 'A3', 'A4', 'A5'}
    for case in catalogue['cases']:
        assert set(case['acceptance']) <= acceptance
        assert case['oracle'] and case['workload']
        assert case['status'] == 'planned_not_executed'
    mapped = {a: [c['id'] for c in catalogue['cases'] if a in c['acceptance']]
              for a in sorted(acceptance)}
    assert all(mapped.values())
    all_geometry = []
    for g in layout['granules']:
        m = ((640+g-1)//g)*g
        emin = ((max(layout['erase_min'],m+g)+g-1)//g)*g
        count = 0
        for e in range(emin,layout['erase_max']+1,g):
            assert 640 <= m and m % g == 0 and m+g <= e and m+g <= 1024
            count += 1
        assert not (m+g <= 512)
        all_geometry.append({'G':g,'minimum_E':emin,'legal_E_count':count,
                             'maximum_save_commands':3+m//g+4*((65536+255)//256)})
    assert max(x['maximum_save_commands'] for x in all_geometry) == 1667
    summaries = []
    for geometry in catalogue['geometries']:
        g, e = geometry['G'], geometry['E']
        m = ((640 + g - 1) // g) * g
        assert g in layout['granules'] and e % g == 0 and m + g <= e
        labels = list(prefixes(g, e))
        assert len(labels) == len(set(labels)) == e + 1 + (m // g + 1) * (g + 1)
        assert ('erase', 0, 0) in labels and ('erase', 0, e) in labels
        assert ('commit', m, 0) in labels and ('commit', m, g) in labels
        for offset in range(0, m, g):
            assert ('body', offset, 0) in labels and ('body', offset, g) in labels
        serial = ''.join(f'{a},{b},{c}\n' for a, b, c in labels).encode()
        reads = 4 * ((e + 255) // 256)
        summaries.append({**geometry, 'M': m, 'body_programs': m // g,
                          'marker_programs': 1, 'erase_commands': 1,
                          'preflight_plus_verification_reads': reads,
                          'commands_including_wear': 1 + 1 + m // g + 1 + reads,
                          'prefix_labels_per_initial_state': len(labels),
                          'prefix_labels_all_three_initial_states': len(labels) * 3,
                          'ordered_labels_sha256': hashlib.sha256(serial).hexdigest(),
                          'boundary_examples': [labels[0], labels[e], labels[e+1], labels[-1]]})
    result = {'status': 'passed_design_checks_only', 'product_tests_executed': 0,
              'power_loss_proof': False, 'layout_negative_controls': negative,
              'planned_cases': len(ids), 'acceptance_mapping': mapped,
              'all_legal_geometry_checks': all_geometry,
              'geometries': summaries,
              'total_planned_prefix_runs': sum(x['prefix_labels_all_three_initial_states'] for x in summaries)}
    print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()
