"""Expected failing assertion for the fully validated erase witness.
First run erase_counterexample.py, which validates all R2/R3 fields and two CRC
implementations. This final assertion applies the R3 ordering and A1 oracle.
"""
from pathlib import Path
import struct
R=Path(__file__).resolve().parents[1]
a=(R/'logs/active-a.bin').read_bytes()
b=(R/'logs/inactive-b-torn-erase.bin').read_bytes()
n=(R/'logs/intended-new.bin').read_bytes()
seq=lambda data:struct.unpack_from('<Q',data,16)[0]
chosen=max([a,b],key=seq)
print('R3 selected sequence',seq(chosen),'payload',chosen[64:68].hex())
assert chosen[64:68] in (a[64:68],n[64:68]), 'R3 recovered neither selected old nor captured new payload'
