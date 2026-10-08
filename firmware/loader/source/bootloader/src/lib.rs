#![no_std]
//! ABI v1 candidate. Hardware adapter must enforce these bounds independently.
//! Standard XCP opcodes/layouts: pyXCP reference 016cf3e, types.py/master.py.
//! No block mode, compression, authentication, DAQ or calibration resource.
pub const APP: u32 = 0x8000;
pub const META: u32 = 0x3f800;
pub const SECTOR: u32 = 2048;
pub const RAM_START: u32 = 0x20000000;
pub const RAM_END: u32 = 0x20018000;
pub const BOARD: u32 = 0x13e23019;
pub const ID: &[u8] = b"AM13E23019-CANBOOT/0.1.0/ABI1";
pub const MAX_CTO: usize = 64;
pub const ID_MTA: u32 = 0xffff0000;
const SYNTAX: u8 = 0x21;
const RANGE: u8 = 0x22;
const SEQUENCE: u8 = 0x29;
const VERIFY: u8 = 0x32;
const GENERIC: u8 = 0x31;

pub fn crc32(bytes: &[u8]) -> u32 { !crc_update(!0, bytes) }
fn crc_update(mut crc: u32, bytes: &[u8]) -> u32 {
    for &b in bytes {
        crc ^= b as u32;
        for _ in 0..8 { crc = (crc >> 1) ^ (0xedb88320 & 0u32.wrapping_sub(crc & 1)); }
    }
    crc
}
fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at+4].try_into().unwrap())
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header { pub len: u32, pub crc: u32, pub version: u32 }
impl Header {
    pub fn decode(b: &[u8;32]) -> Option<Self> {
        if &b[..4] != b"AB01" || word(b,4) != BOARD || word(b,8) != 1
            || word(b,12) != APP || word(b,28) != 0 { return None; }
        let len=word(b,16);
        if len < 8 || len > META-APP || len % 16 != 0 { return None; }
        Some(Self { len, crc:word(b,20), version:word(b,24) })
    }
    pub fn erase_len(self) -> u32 { (self.len + SECTOR-1) & !(SECTOR-1) }
    pub fn vectors_valid(self, sp:u32, pc:u32) -> bool {
        sp > RAM_START && sp <= RAM_END && sp % 8 == 0 && pc & 1 == 1
            && (pc & !1) >= APP+8 && (pc & !1) < APP+self.len
    }
}
/// Must implement bounded, ECC-aware reads and 16-byte program-once writes.
/// Hardware failure fences all mutations until reset; no automatic retries.
/// invalidate() erases ONLY META sector and verifies it BEFORE app erase begins.
/// commit() writes header, verifies, then a separate final 16-byte marker granule.
/// RAM flash command closure, blank/ECC handling and reset recovery are adapter duties.
pub trait Flash {
    fn read(&mut self, address:u32, output:&mut [u8]) -> Result<(),()>;
    fn invalidate(&mut self) -> Result<(),()>;
    fn erase_app(&mut self, length:u32) -> Result<(),()>;
    fn program_word(&mut self, address:u32, data:&[u8;16]) -> Result<(),()>;
    fn commit(&mut self, header:&[u8;32]) -> Result<(),()>;
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase { Idle, Header, Ready, Writing, Complete, Fault }
#[derive(Debug)]
pub struct Reply { pub bytes:[u8;MAX_CTO], pub len:usize, pub reset:bool }
fn reply(data:&[u8]) -> Reply {
    let mut bytes=[0;MAX_CTO];bytes[..data.len()].copy_from_slice(data);
    Reply { bytes, len:data.len(), reset:false }
}
fn error(code:u8) -> Reply { reply(&[0xfe,code]) }
pub struct Session {
    pub connected:bool, pub phase:Phase, mta:u32, header:[u8;32], header_used:usize,
    image:Option<Header>, written:u32, buffered:usize, buffer:[u8;16],
}
impl Default for Session { fn default()->Self { Self::new() } }
impl Session {
    pub const fn new()->Self {
        Self { connected:false,phase:Phase::Idle,mta:0,header:[0;32],header_used:0,
            image:None,written:0,buffered:0,buffer:[0;16] }
    }
    fn fail(&mut self)->Reply { self.phase=Phase::Fault;error(GENERIC) }
    /// Transport filters standard-ID CAN-FD BRS frames at ID0x710, payload length1..64.
    /// Extra bytes up to MAX_CTO are CAN padding; requested counts must fit DLC.
    pub fn command<F:Flash>(&mut self, p:&[u8], flash:&mut F)->Option<Reply> {
        if p.is_empty() || p.len()>MAX_CTO { return None; }
        let need=match p[0] {
            0xff|0xfa|0xf5|0xd0 => 2,
            0xf6|0xd1 => 8,
            _=>1,
        };
        if p.len()<need { return Some(error(SYNTAX)); }
        if p[0]==0xff {
            if p[1]!=0 { return Some(error(RANGE)); }
            // Reconnect never discards a partial/faulted programming transaction.
            self.connected=true;
            return Some(reply(&[0xff,0x10,0,64,64,0,1,1]));
        }
        if !self.connected { return None; }
        let r=match p[0] {
            0xfe => {self.connected=false;reply(&[0xff])},
            0xfd => reply(&[0xff,0,0,0,0,0]),
            0xfc => error(0), // SYNCH always ERR_CMD_SYNCH
            0xfa => {
                if p[1]!=0 { error(RANGE) } else {
                    self.mta=ID_MTA;
                    let mut r=reply(&[0xff,0,0,0,0,0,0,0]);
                    r.bytes[4..8].copy_from_slice(&(ID.len() as u32).to_le_bytes());r
                }
            },
            0xf6 => {
                if p[3]!=0 { error(RANGE) }
                else { self.mta=word(p,4);reply(&[0xff]) }
            },
            0xf5 => self.upload(p[1] as usize,flash),
            0xce => reply(&[0xff,1,0]), // absolute, sequential, no sector-info implementation
            0xd2 => {
                if self.phase!=Phase::Idle { error(SEQUENCE) }
                else { self.phase=Phase::Header;reply(&[0xff,0,0,64,0,0,0]) }
            },
            0xd1 => self.clear(p,flash),
            0xd0 => self.program(p,flash),
            0xcf => self.finish(flash),
            _ => error(0x20),
        };
        Some(r)
    }
    fn upload<F:Flash>(&mut self,n:usize,flash:&mut F)->Reply {
        if n==0 || n>MAX_CTO-1 { return error(RANGE); }
        let Some(end)=self.mta.checked_add(n as u32) else {return error(RANGE)};
        let mut r=reply(&[0xff]);r.len=n+1;
        if self.mta>=ID_MTA && end<=ID_MTA+ID.len() as u32 {
            let at=(self.mta-ID_MTA) as usize;r.bytes[1..n+1].copy_from_slice(&ID[at..at+n]);
        } else if end<=0x80000 {
            if flash.read(self.mta,&mut r.bytes[1..n+1]).is_err() {return error(VERIFY)}
        } else {return error(RANGE)}
        self.mta=end;r
    }
    fn clear<F:Flash>(&mut self,p:&[u8],flash:&mut F)->Reply {
        if self.phase!=Phase::Ready {return error(SEQUENCE)}
        let h=self.image.unwrap();
        if p[1]!=0 || self.mta!=APP || word(p,4)!=h.erase_len() {return error(RANGE)}
        if flash.invalidate().is_err() {return self.fail()}
        if flash.erase_app(h.erase_len()).is_err() {return self.fail()}
        self.phase=Phase::Writing;reply(&[0xff])
    }
    fn program<F:Flash>(&mut self,p:&[u8],flash:&mut F)->Reply {
        let n=p[1] as usize;
        if n>MAX_CTO-2 || p.len()<n+2 {return error(SYNTAX)}
        if self.phase==Phase::Header {
            if n==0 || self.mta!=META+self.header_used as u32 || self.header_used+n>32 {return error(RANGE)}
            self.header[self.header_used..self.header_used+n].copy_from_slice(&p[2..2+n]);
            self.header_used+=n;self.mta+=n as u32;
            if self.header_used==32 {
                self.image=Header::decode(&self.header);
                if self.image.is_none() {self.phase=Phase::Fault;return error(VERIFY)}
                self.phase=Phase::Ready;
            }
            return reply(&[0xff]);
        }
        if self.phase!=Phase::Writing {return error(SEQUENCE)}
        let h=self.image.unwrap();
        if self.mta!=APP+self.written {return error(SEQUENCE)}
        if n==0 {
            if self.written!=h.len || self.buffered!=0 {return error(SEQUENCE)}
            if !verify_image(flash,h) {self.phase=Phase::Fault;return error(VERIFY)}
            self.phase=Phase::Complete;return reply(&[0xff]);
        }
        if self.written+n as u32>h.len {return error(RANGE)}
        for &b in &p[2..2+n] {
            self.buffer[self.buffered]=b;self.buffered+=1;self.written+=1;self.mta+=1;
            if self.buffered==16 {
                if flash.program_word(APP+self.written-16,&self.buffer).is_err() {return self.fail()}
                self.buffered=0;
            }
        }
        reply(&[0xff])
    }
    fn finish<F:Flash>(&mut self,flash:&mut F)->Reply {
        if self.phase!=Phase::Complete {return error(SEQUENCE)}
        if !verify_image(flash,self.image.unwrap()) {self.phase=Phase::Fault;return error(VERIFY)}
        if flash.commit(&self.header).is_err() {return self.fail()}
        // Adapter transmits ACK completely before issuing SYSRESETREQ.
        let mut r=reply(&[0xff]);r.reset=true;r
    }
}
pub fn verify_image<F:Flash>(flash:&mut F,h:Header)->bool {
    let mut vector=[0;8];
    if flash.read(APP,&mut vector).is_err() || !h.vectors_valid(word(&vector,0),word(&vector,4)) {return false}
    let mut crc=!0;let mut buffer=[0;64];let mut at=0;
    while at<h.len {
        let n=core::cmp::min(64,h.len-at) as usize;
        if flash.read(APP+at,&mut buffer[..n]).is_err() {return false}
        crc=crc_update(crc,&buffer[..n]);at+=n as u32;
    }
    !crc==h.crc
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::{vec,vec::Vec};
    struct Model { mem:Vec<u8>, ops:Vec<(u8,u32)>, valid:bool, fail_invalidate:bool }
    impl Model {fn new()->Self {Self {mem:vec![0xff;0x80000],ops:Vec::new(),valid:true,fail_invalidate:false}}}
    impl Flash for Model {
        fn read(&mut self,a:u32,o:&mut[u8])->Result<(),()> {o.copy_from_slice(&self.mem[a as usize..a as usize+o.len()]);Ok(())}
        fn invalidate(&mut self)->Result<(),()> {self.ops.push((0,META));if self.fail_invalidate{return Err(())}self.valid=false;Ok(())}
        fn erase_app(&mut self,n:u32)->Result<(),()> {assert!(!self.valid);assert!(n%SECTOR==0 && n<=META-APP);self.ops.push((1,n));self.mem[APP as usize..(APP+n) as usize].fill(0xff);Ok(())}
        fn program_word(&mut self,a:u32,b:&[u8;16])->Result<(),()> {
            assert!(a>=APP && a+16<=META && a%16==0);assert!(!self.valid);
            assert!(self.mem[a as usize..a as usize+16].iter().all(|&v|v==0xff));
            self.ops.push((2,a));self.mem[a as usize..a as usize+16].copy_from_slice(b);Ok(())
        }
        fn commit(&mut self,_:&[u8;32])->Result<(),()> {self.ops.push((3,META));self.valid=true;Ok(())}
    }
    fn send(s:&mut Session,f:&mut Model,p:&[u8])->Reply {s.command(p,f).unwrap()}
    fn ok(s:&mut Session,f:&mut Model,p:&[u8]) {assert_eq!(send(s,f,p).bytes[0],0xff,"{p:x?}");}
    fn mta(s:&mut Session,f:&mut Model,a:u32) {let mut p=[0xf6,0,0,0,0,0,0,0];p[4..].copy_from_slice(&a.to_le_bytes());ok(s,f,&p);}
    fn program(s:&mut Session,f:&mut Model,b:&[u8]) {for c in b.chunks(6) {let mut p=vec![0xd0,c.len() as u8];p.extend(c);ok(s,f,&p);}}
    fn image()->([u8;32],[u8;32]) {
        let mut data=[0x55;32];data[..4].copy_from_slice(&RAM_END.to_le_bytes());data[4..8].copy_from_slice(&(APP+9).to_le_bytes());
        let mut h=[0;32];h[..4].copy_from_slice(b"AB01");
        for (i,v) in [BOARD,1,APP,32,crc32(&data),1,0].into_iter().enumerate(){h[4+i*4..8+i*4].copy_from_slice(&v.to_le_bytes());}(h,data)
    }
    fn ready(s:&mut Session,f:&mut Model)->[u8;32] {
        let(h,d)=image();ok(s,f,&[0xff,0]);ok(s,f,&[0xd2]);mta(s,f,META);program(s,f,&h);assert_eq!(s.phase,Phase::Ready);d
    }
    fn clear(s:&mut Session,f:&mut Model) {mta(s,f,APP);ok(s,f,&[0xd1,0,0,0,0,8,0,0]);}
    #[test] fn complete_commit_last_and_calibration_untouched() {
        let mut s=Session::new();let mut f=Model::new();f.mem[0x78000]=0x42;
        let d=ready(&mut s,&mut f);assert!(f.ops.is_empty());clear(&mut s,&mut f);program(&mut s,&mut f,&d);
        assert!(!f.valid);ok(&mut s,&mut f,&[0xd0,0]);assert!(!f.valid);
        assert!(send(&mut s,&mut f,&[0xcf]).reset);assert!(f.valid);assert_eq!(f.mem[0x78000],0x42);
        assert_eq!(f.ops,[(0,META),(1,2048),(2,APP),(2,APP+16),(3,META)]);
    }
    #[test] fn invalidation_failure_never_erases_application() {
        let mut s=Session::new();let mut f=Model::new();ready(&mut s,&mut f);f.fail_invalidate=true;mta(&mut s,&mut f,APP);
        assert_eq!(send(&mut s,&mut f,&[0xd1,0,0,0,0,8,0,0]).bytes[1],GENERIC);
        assert_eq!(f.ops,[(0,META)]);assert_eq!(s.phase,Phase::Fault);
    }
    #[test] fn interruption_and_early_reset_never_commit() {
        let mut s=Session::new();let mut f=Model::new();let d=ready(&mut s,&mut f);clear(&mut s,&mut f);
        program(&mut s,&mut f,&d[..18]);assert_eq!(send(&mut s,&mut f,&[0xcf]).bytes[1],SEQUENCE);
        assert_eq!(send(&mut s,&mut f,&[0xd0,0]).bytes[1],SEQUENCE);assert!(!f.valid);
        let mut restarted=Session::new();ok(&mut restarted,&mut f,&[0xff,0]);assert!(restarted.connected);assert!(!f.valid);
    }
    #[test] fn corruption_fails_crc_and_bad_vectors_fail() {
        let mut s=Session::new();let mut f=Model::new();let mut d=ready(&mut s,&mut f);clear(&mut s,&mut f);d[20]^=1;program(&mut s,&mut f,&d);
        assert_eq!(send(&mut s,&mut f,&[0xd0,0]).bytes[1],VERIFY);assert!(!f.valid);
        let h=Header::decode(&image().0).unwrap();
        for(sp,pc)in[(RAM_START,APP+9),(RAM_END+8,APP+9),(RAM_END-1,APP+9),(RAM_END,APP+8),(RAM_END,APP+33),(RAM_END,APP-1)]{assert!(!h.vectors_valid(sp,pc));}
    }
    #[test] fn malformed_out_of_order_and_loader_writes_have_no_side_effects() {
        let mut s=Session::new();let mut f=Model::new();assert!(s.command(&[0xd2],&mut f).is_none());ok(&mut s,&mut f,&[0xff,0]);
        for p in [&[0xd0,1,0][..],&[0xcf],&[0xd1,0,0,0,0,8,0,0]] {assert_eq!(send(&mut s,&mut f,p).bytes[1],SEQUENCE);}
        assert_eq!(send(&mut s,&mut f,&[0xf6]).bytes[1],SYNTAX);
        ok(&mut s,&mut f,&[0xd2]);mta(&mut s,&mut f,0);assert_eq!(send(&mut s,&mut f,&[0xd0,1,0]).bytes[1],RANGE);assert!(f.ops.is_empty());
    }
    #[test] fn header_board_layout_length_and_program_order() {
        let(h,_)=image();for offset in [0,4,8,12,16,28] {let mut b=h;b[offset]^=1;assert!(Header::decode(&b).is_none());}
        let mut s=Session::new();let mut f=Model::new();let d=ready(&mut s,&mut f);clear(&mut s,&mut f);mta(&mut s,&mut f,APP+16);
        assert_eq!(send(&mut s,&mut f,&[0xd0,1,0]).bytes[1],SEQUENCE);mta(&mut s,&mut f,APP);program(&mut s,&mut f,&d);
        assert_eq!(send(&mut s,&mut f,&[0xd0,1,0]).bytes[1],RANGE);
    }
    #[test] fn fd_full_frame_and_padding_count() {
        let mut s=Session::new();let mut f=Model::new();let (mut h,_)=image();
        let mut data=[0x44;80];data[..4].copy_from_slice(&RAM_END.to_le_bytes());data[4..8].copy_from_slice(&(APP+9).to_le_bytes());
        h[16..20].copy_from_slice(&80u32.to_le_bytes());h[20..24].copy_from_slice(&crc32(&data).to_le_bytes());
        ok(&mut s,&mut f,&[0xff,0]);ok(&mut s,&mut f,&[0xd2]);mta(&mut s,&mut f,META);program(&mut s,&mut f,&h);clear(&mut s,&mut f);
        let mut p=[0;64];p[0]=0xd0;p[1]=62;p[2..].copy_from_slice(&data[..62]);ok(&mut s,&mut f,&p);
        // A padded32-byte CAN-FD payload carries only18 declared data bytes.
        let mut tail=[0xcc;32];tail[0]=0xd0;tail[1]=18;tail[2..20].copy_from_slice(&data[62..]);ok(&mut s,&mut f,&tail);
        ok(&mut s,&mut f,&[0xd0,0]);assert!(send(&mut s,&mut f,&[0xcf]).reset);
        assert_eq!(&f.mem[APP as usize..APP as usize+80],&data);
    }
    #[test] fn crc_reference_and_protocol_sizes() {
        assert_eq!(crc32(b"123456789"),0xcbf43926);let mut s=Session::new();let mut f=Model::new();
        assert_eq!(&send(&mut s,&mut f,&[0xff,0]).bytes[..8],&[0xff,0x10,0,64,64,0,1,1]);
        let r=send(&mut s,&mut f,&[0xd2]);assert_eq!(r.len,7);assert_eq!(&r.bytes[..7],&[0xff,0,0,64,0,0,0]);
    }
}
