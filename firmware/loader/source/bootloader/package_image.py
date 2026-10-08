#!/usr/bin/env python3
"""ABI1 package construction/validation; no device access."""
import argparse, pathlib, struct, zlib
APP=0x8000
META=0x3f800
BOARD=0x13e23019
HEADER=struct.Struct('<4s7I')
def validate(package):
    if len(package)<40: raise ValueError('truncated package')
    magic,board,abi,base,length,crc,version,reserved=HEADER.unpack(package[:32])
    image=package[32:]
    if (magic,board,abi,base,reserved)!=(b'AB01',BOARD,1,APP,0): raise ValueError('board/layout/ABI mismatch')
    if length!=len(image) or not 16<=length<=META-APP or length%16: raise ValueError('length/alignment')
    if zlib.crc32(image)!=crc: raise ValueError('CRC mismatch')
    sp,pc=struct.unpack_from('<II',image)
    if not (0x20000000<sp<=0x20018000 and sp%8==0 and pc&1 and APP+8<=pc&~1<APP+length): raise ValueError('vector bounds')
    return package[:32],image

def build(image,version):
    image+=b'\xff'*((-len(image))%16)
    package=HEADER.pack(b'AB01',BOARD,1,APP,len(image),zlib.crc32(image),version,0)+image
    validate(package)
    return package
if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('app',type=pathlib.Path);p.add_argument('output',type=pathlib.Path);p.add_argument('--version',type=int,required=True)
    a=p.parse_args();a.output.write_bytes(build(a.app.read_bytes(),a.version))
