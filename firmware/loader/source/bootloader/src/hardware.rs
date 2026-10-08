use am13_can_boot_core::{APP,META,SECTOR,Header,Flash,crc32,verify_image};
use core::ptr::{read_volatile as rd,write_volatile as wr};
#[inline(always)] unsafe fn read(a:usize)->u32 {unsafe{rd(a as *const u32)}}
#[inline(always)] unsafe fn write(a:usize,v:u32) {unsafe{wr(a as *mut u32,v)}}

/// No flash references, calls, tables or interrupt dependencies while ECC disabled.
/// TI SPRUJF2B13.5.6 requires RAM closure and nine cycles after FRI configuration.
/// Reads intentionally use raw data + image/header CRC, allowing partial ECC granules
/// left by power loss to be rejected without repeatedly faulting at boot.
#[unsafe(link_section=".data.ram_read")]
#[inline(never)]
unsafe fn raw_read(address:usize,out:*mut u8,len:usize) {
    unsafe {
        let ecc=read(0x40029100);
        write(0x40029100,0);
        core::arch::asm!("dsb", "isb", "nop", "nop", "nop", "nop", "nop", "nop", "nop", "nop", "nop",options(nostack));
        let mut i=0;
        while i<len {wr(out.add(i),rd((address+i) as *const u8));i+=1;}
        write(0x40029100,ecc);
        core::arch::asm!("dsb", "isb", "nop", "nop", "nop", "nop", "nop", "nop", "nop", "nop", "nop",options(nostack));
    }
}

/// Exact command fields/GSC lifecycle from the working calibration sequencer.
/// Supports bank0 app/metadata only; all argument checks precede command MMIO.
/// Dynamic protection uses SDK sector map plus documented alternative B group;
/// physical addresses stay exact and loader-sector bits0..15 remain protected.
#[unsafe(link_section=".data.ram_flash")]
#[inline(never)]
unsafe fn media(address:u32,erase:bool,data:*const u8)->u32 {
    unsafe {
        if address<APP || address>=META+SECTOR || (!erase && address%16!=0)
            || (erase && address%SECTOR!=0) {return 0xffff0001}
        write(0x40047800,1);
        if read(0x40047808)&0xc0000000!=0xc0000000 {return 0xffff0002}
        let factory=read(0x60111074);
        if factory&0xfff!=512 || (factory>>12)&3!=2 || read(0x4002900c)&4!=0
            || read(0x400b2048)&0x1000!=0 || read(0x40043210)&3!=3
            || read(0x400433d0)&4!=0 {
            write(0x40047804,1);return 0xffff0003
        }
        // SYSOSC32MHz requires at least one flash wait state; configure in RAM.
        let wait=read(0x40029000);
        if (wait>>8)&15==0 {
            write(0x40029000,wait|0x100);
            core::arch::asm!("dsb","isb","nop","nop","nop","nop","nop","nop","nop","nop","nop",options(nostack));
        }
        let mut result=0;
        // Clear status first; it resets command protections. Then program/erase.
        let mut pass=0;
        while pass<2 {
            if pass==1 {
                let sector=address/SECTOR;
                let a=if sector<32 {!(1u32<<sector)} else {u32::MAX};
                let b=if sector<32 {0xffff} else {0xffff & !(1u32<<(sector/8-4)) & !(1u32<<(sector/8))};
                write(0x400431d0,a);write(0x400431d4,b);
                if read(0x40043210)&3!=3 {result=0xffff0004;break}
            }
            write(0x40043104,if pass==0 {5} else if erase {0x42} else {1});
            write(0x40043108,0);
            write(0x40043120,if pass==0 {0} else {address});
            write(0x40043124,if pass==1 && !erase {0x3ffff} else {0});
            write(0x4004312c,if pass==0 {0} else {(address>>4)&3});
            let mut n=0;
            while n<4 {
                let value=if pass==1 && !erase {
                    (rd(data.add(n*4)) as u32)|((rd(data.add(n*4+1))as u32)<<8)|((rd(data.add(n*4+2))as u32)<<16)|((rd(data.add(n*4+3))as u32)<<24)
                } else {0};
                write(0x40043130+n*4,value);n+=1;
            }
            core::arch::asm!("dsb","isb",options(nostack));
            write(0x40043100,1);
            core::arch::asm!("dsb","isb",options(nostack));
            let mut remaining=4_000_000u32;
            let status=loop {
                let s=read(0x400433d0);
                if s&4==0 {break s}
                remaining-=1;
                if remaining==0 {
                    // Never return into busy flash. Bounded command watchdog reset.
                    write(0xe000ed0c,0x05fa0004);
                    core::arch::asm!("dsb",options(nostack));
                    loop {core::arch::asm!("nop",options(nostack))}
                }
            };
            if status&0x11f0!=0 || (pass==1 && status&3!=3) {result=status|0x80000000;break}
            pass+=1;
        }
        write(0x400431d0,u32::MAX);write(0x400431d4,0xffff);
        write(0x40047804,1);
        result
    }
}

pub struct Storage { pub failed:bool, pub last_status:u32 }
impl Storage {
    pub const fn new()->Self {Self{failed:false,last_status:0}}
    fn run(&mut self,a:u32,erase:bool,b:&[u8;16])->Result<(),()> {
        if self.failed {return Err(())}
        self.last_status=unsafe{media(a,erase,b.as_ptr())};
        if self.last_status!=0 {self.failed=true;return Err(())}Ok(())
    }
    fn erase(&mut self,a:u32)->Result<(),()> {
        self.run(a,true,&[0;16])?;
        let mut b=[0;64];
        for off in (0..SECTOR).step_by(64) {
            self.read(a+off,&mut b)?;
            if b!=[0xff;64] {self.failed=true;return Err(())}
        }Ok(())
    }
    fn word(&mut self,a:u32,b:&[u8;16])->Result<(),()> {
        self.run(a,false,b)?;let mut out=[0;16];self.read(a,&mut out)?;
        if out!=*b {self.failed=true;return Err(())}Ok(())
    }
    pub fn valid(&mut self)->Option<Header> {
        let mut bytes=[0;48];self.read(META,&mut bytes).ok()?;
        if &bytes[32..40]!=b"COMMIT01" {return None}
        let crc=u32::from_le_bytes(bytes[40..44].try_into().ok()?);
        let inv=u32::from_le_bytes(bytes[44..48].try_into().ok()?);
        if crc!=!inv || crc32(&bytes[..32])!=crc {return None}
        let h=Header::decode(bytes[..32].try_into().ok()?)?;
        if verify_image(self,h) {Some(h)} else {None}
    }
}
impl Flash for Storage {
    fn read(&mut self,a:u32,out:&mut[u8])->Result<(),()> {
        if a.checked_add(out.len()as u32).is_none_or(|end|end>0x80000) {return Err(())}
        unsafe{raw_read(a as usize,out.as_mut_ptr(),out.len())};Ok(())
    }
    fn invalidate(&mut self)->Result<(),()> {self.erase(META)}
    fn erase_app(&mut self,len:u32)->Result<(),()> {
        if len==0 || len%SECTOR!=0 || len>META-APP {return Err(())}
        for a in (APP..APP+len).step_by(SECTOR as usize) {self.erase(a)?;}Ok(())
    }
    fn program_word(&mut self,a:u32,b:&[u8;16])->Result<(),()> {
        if a<APP || a.checked_add(16).is_none_or(|end|end>META) || a%16!=0 {return Err(())}
        self.word(a,b)
    }
    fn commit(&mut self,header:&[u8;32])->Result<(),()> {
        let h=Header::decode(header).ok_or(())?;
        if !verify_image(self,h) {return Err(())}
        self.word(META,header[..16].try_into().unwrap())?;
        self.word(META+16,header[16..].try_into().unwrap())?;
        let mut marker=[0;16];marker[..8].copy_from_slice(b"COMMIT01");let crc=crc32(header);
        marker[8..12].copy_from_slice(&crc.to_le_bytes());marker[12..].copy_from_slice(&(!crc).to_le_bytes());
        self.word(META+32,&marker)?;
        if self.valid()!=Some(h) {self.failed=true;return Err(())}Ok(())
    }
}
