//! MCAN0 classical 11-bit data frames, one RX FIFO element and one TX buffer.
//! Register layout and message RAM encoding follow TI SDK 26_01_00_03 `dl_mcan`.
//! Classic CAN uses25MHz XTAL/2, satisfying MCAN_ICLK>=MCAN_FCLK at32MHz CPU.
use crate::io::RegisterIo;
const B: usize = 0x4011_0000;
const S: usize = 0x400a_f000;
const C: usize = B + 0x7000;
const TX: usize = B + 0x10; // RX FIFO0 occupies 0x00..0x0f; TX0 0x10..0x1f
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidBitrate,
    InvalidFrame,
    Clock,
    Power,
    Timeout,
    BusOff,
    Protocol,
    UnsupportedFrame,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame {
    pub id: u16,
    pub len: u8,
    pub data: [u8; 8],
}
impl Frame {
    pub fn new(id: u16, payload: &[u8]) -> Result<Self, Error> {
        if id > 0x7ff || payload.len() > 8 {
            return Err(Error::InvalidFrame);
        }
        let mut data = [0; 8];
        data[..payload.len()].copy_from_slice(payload);
        Ok(Self {
            id,
            len: payload.len() as u8,
            data,
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Status {
    pub bus_off: bool,
    pub tx_errors: u8,
    pub rx_errors: u8,
    pub last_error: u8,
    pub rx_fifo_lost: bool,
    pub message_ram_error: bool,
}
pub struct Mcan0<I> {
    io: I,
}
pub struct Can<I> {
    io: I,
    pub nominal_bitrate: u32,
}
impl<I: RegisterIo> Mcan0<I> {
    pub(crate) fn new(io: I) -> Self {
        Self { io }
    }
    /// 125, 250 or 500 kbps; 80% nominal sample point from a 25 MHz crystal.
    pub fn configure(self, bitrate: u32) -> Result<Can<I>, Error> {
        let brp = match bitrate {
            125_000 => 4,
            250_000 => 2,
            500_000 => 1,
            _ => return Err(Error::InvalidBitrate),
        };
        //12.5MHz/(BRP*25);80% sample point at all three classical rates.
        let (seg1, seg2) = (19, 5);
        // XTAL source from v3 PC16_X1/PC17_X2, 25 MHz on SoM.
        self.io.modify(S + 0x1474, 0, 1);
        self.io.modify(S + 0x1110, 0xff, 0xff); // maximum 16 ms startup allowance
        self.io.modify(S + 0x1474, 1, 0);
        self.io.modify(S + 0x1110, 0, 1 << 28); // startup monitor
        if !self.io.wait(S + 0x1204, 1 << 8, 1 << 8) {
            return Err(Error::Clock);
        }
        self.io.modify(S + 0x1140, 1 << 8, 0); // CANCLK = HFCLK/XTAL
        self.io.write(B + 0x6804, 0xb100_0003);
        self.io.write(B + 0x6800, 0x2600_0001);
        self.io.delay_cycles(100);
        if self.io.read(B + 0x6800) & 1 == 0 {
            return Err(Error::Power);
        }
        self.io.write(B + 0x7908, 0); // CLKCTL.STOPREQ=0
        self.io.write(B + 0x7900, 1); // CLKEN.CLK_REQEN=1 (distinct from CLKCTL)
        self.io.write(B + 0x7904, 1); // latched only after CLKEN; XTAL25MHz/2
        if self.io.read(B + 0x7904) != 1 { return Err(Error::Clock); }
        self.io.delay_cycles(100);
        if !self.io.wait(B + 0x7208, 2, 2) {
            return Err(Error::Timeout);
        } // RAM init done
        self.io.modify(C + 0x18, 0, 1); // INIT
        if !self.io.wait(C + 0x18, 1, 1) {
            return Err(Error::Timeout);
        }
        self.io.modify(C + 0x18, 0, 2); // CCE unlock
        let nbtp = ((brp - 1) << 16) | ((seg1 - 1) << 8) | (seg2 - 1) | ((seg2 - 1) << 25);
        self.io.write(C + 0x1c, nbtp);
        self.io.modify(C + 0x18, (1 << 8) | (1 << 9), 0); // classical only
        self.io.write(C + 0x80, 0x0b); // standard nonmatch -> FIFO0; reject extended/remote
        self.io.write(C + 0xa0, 1 << 16); // RX FIFO0 one 16-byte element at RAM 0
        self.io.write(C + 0xbc, 0); // 8-byte RX element
        self.io.write(C + 0xc0, (1 << 16) | 0x10); // TX0 at RAM offset 0x10
        self.io.write(C + 0xc8, 0); // 8-byte TX element
        self.io.write(C + 0xe0, 1); // TXBTIE0: generate IR.TC on TX0 completion
        for offset in (0..0x20).step_by(4) {
            self.io.write(B + offset, 0);
        }
        self.io.modify(C + 0x18, 3, 0); // normal operation
        if !self.io.wait(C + 0x18, 1, 0) {
            return Err(Error::Timeout);
        }
        Ok(Can {
            io: self.io,
            nominal_bitrate: bitrate,
        })
    }
}
impl<I: RegisterIo> Can<I> {
    pub fn status(&self) -> Status {
        let psr = self.io.read(C + 0x44);
        let ecr = self.io.read(C + 0x40);
        let ir = self.io.read(C + 0x50);
        Status {
            bus_off: psr & (1 << 7) != 0,
            tx_errors: ecr as u8,
            rx_errors: (ecr >> 8) as u8 & 0x7f,
            last_error: psr as u8 & 7,
            rx_fifo_lost: ir & (1 << 3) != 0,
            message_ram_error: ir & (1 << 17) != 0,
        }
    }
    pub fn send(&mut self, frame: Frame) -> Result<(), Error> {
        if frame.id > 0x7ff || frame.len > 8 {
            return Err(Error::InvalidFrame);
        }
        if self.status().bus_off {
            return Err(Error::BusOff);
        }
        if !self.io.wait(C + 0xcc, 1, 0) {
            return Err(Error::Timeout);
        }
        self.io.write(TX, (frame.id as u32) << 18);
        self.io.write(TX + 4, (frame.len as u32) << 16);
        for word in 0..2 {
            let o = word * 4;
            self.io.write(
                TX + 8 + o,
                u32::from_le_bytes([
                    frame.data[o],
                    frame.data[o + 1],
                    frame.data[o + 2],
                    frame.data[o + 3],
                ]),
            );
        }
        self.io.write(C + 0x50, (1 << 9) | (1 << 27) | (1 << 28)); // W1C TX completion/protocol flags
        self.io.write(C + 0xd0, 1);
        for _ in 0..100_000 {
            if self.status().bus_off {
                return Err(Error::BusOff);
            }
            let ir = self.io.read(C + 0x50);
            if ir & ((1 << 17) | (1 << 27) | (1 << 28)) != 0 {
                return Err(Error::Protocol);
            }
            if ir & (1 << 9) != 0 && self.io.read(C + 0xd8) & 1 != 0 {
                return Ok(());
            }
        }
        self.io.write(C + 0xd4, 1); // bounded cancellation request
        Err(Error::Timeout)
    }
    pub fn receive(&mut self) -> Result<Option<Frame>, Error> {
        if self.status().bus_off {
            return Err(Error::BusOff);
        }
        let f0s = self.io.read(C + 0xa4);
        if f0s & 0x7f == 0 {
            return Ok(None);
        }
        let header = self.io.read(B);
        let control = self.io.read(B + 4);
        if header & ((1 << 29) | (1 << 30)) != 0 || control & (1 << 21) != 0 {
            self.io.write(C + 0xa8, 0);
            return Err(Error::UnsupportedFrame);
        }
        let len = ((control >> 16) & 0xf) as u8;
        if len > 8 {
            self.io.write(C + 0xa8, 0);
            return Err(Error::UnsupportedFrame);
        }
        let mut data = [0; 8];
        for word in 0..2 {
            data[word * 4..word * 4 + 4]
                .copy_from_slice(&self.io.read(B + 8 + word * 4).to_le_bytes());
        }
        self.io.write(C + 0xa8, 0);
        Ok(Some(Frame {
            id: ((header >> 18) & 0x7ff) as u16,
            len,
            data,
        }))
    }
}

/// Bootloader-only CAN-FD profile. Existing classical application profile unchanged.
/// Starts on SYSOSC32MHz; keeps CPU32MHz and uses CANCLK10MHz.
/// 25MHz HFCLK*16=400MHz VCO, /20=20MHz CLK0, CAN/2; interface clock16MHz >= CAN10MHz.
/// SDK26.01 dl_sysctl.c + hw_sysctl.h, TRM3.4.2.3; bounded lock/start waits.
pub struct FdCan<I> { inner: Can<I> }
#[derive(Clone, Copy, Debug)]
pub struct FdFrame { pub id:u16, pub len:usize, pub data:[u8;64] }
pub const FD_LENGTHS:[usize;16]=[0,1,2,3,4,5,6,7,8,12,16,20,24,32,48,64];
impl<I:RegisterIo> Mcan0<I> {
    pub fn configure_boot_fd(self)->Result<FdCan<I>,Error> {
        let can=self.configure(500_000)?;
        let io=&can.io;
        io.modify(C+0x18,0,1);
        if !io.wait(C+0x18,1,1) {return Err(Error::Timeout)}
        io.modify(C+0x18,0,2);
        // Stop MCAN functional clock while its source is reconfigured.
        io.write(B+0x7908,1);
        if io.read(S+0x1104)&(1<<16)!=0 {return Err(Error::Clock)}
        if !(0..100_000).any(|_|io.read(S+0x1204)&((1<<9)|(1<<14))!=0) {return Err(Error::Clock)}
        io.modify(S+0x1108,1<<8,0);
        if !io.wait(S+0x1204,1<<14,1<<14) {return Err(Error::Clock)}
        io.modify(S+0x1120,0xff31,0x911); // HFCLK ref, CLK0 /20 enabled; CLK1 disabled
        io.modify(S+0x1124,0x7f03,15<<8); // PDIV /1; feedback16
        io.write(S+0x1128,io.read(0x60111030)); // input25MHz FACTORY16..32MHz
        io.write(S+0x112c,io.read(0x60111034));
        io.modify(S+0x1108,0,1<<8);
        if !io.wait(S+0x1204,1<<9,1<<9) {return Err(Error::Clock)}
        io.modify(S+0x113c,0xf000,0x8000); // CAN external divide2
        io.modify(S+0x1140,1<<8,1<<8); // CAN source SYSPLLCLK0
        // TRM27.4.2 requires interface>=functional:16MHz>=10MHz.
        // TRM27.5.3 permits data bits >=4tq; here5tq gives2Mbps,80%.
        io.write(B+0x7904,0); // no additional module divider
        io.write(B+0x7908,0);
        io.delay_cycles(100);
        io.modify(C+0x18,0,1);
        if !io.wait(C+0x18,1,1) {return Err(Error::Timeout)}
        io.modify(C+0x18,0,2);
        if !io.wait(C+0x18,3,3) {return Err(Error::Timeout)}
        io.write(C+0x1c,(14<<8)|3|(3<<25)); //10M/(1*20)=500k,80%
        io.write(C+0x0c,(1<<23)|(2<<8)); //10M/(1*5)=2M,80%,SJW1
        io.write(C+0x48,4<<8); // TDC secondary sampling at data sample point4mtq
        io.modify(C+0x18,1<<6,(1<<8)|(1<<9)); // normal CAN error retry, FDOE, BRSE
        io.write(C+0xa0,1<<16); // one72-byte RX element at0
        io.write(C+0xbc,7); // F0DS=64
        io.write(C+0xc0,(1<<16)|0x48); // TX0 byte offset72
        io.write(C+0xc8,7); // TBDS=64
        for at in (0..144).step_by(4) {io.write(B+at,0)}
        if io.read(C+0x1c)!=((14<<8)|3|(3<<25))
            || io.read(C+0x0c)!=((1<<23)|(2<<8))
            || io.read(C+0xc0)!=((1<<16)|0x48)
            || io.read(C+0xbc)!=7 || io.read(C+0xc8)!=7
            || io.read(S+0x1140)&0x100==0 {return Err(Error::Protocol)}
        io.modify(C+0x18,3,0);
        if !io.wait(C+0x18,1,0) {return Err(Error::Timeout)}
        Ok(FdCan {inner:can})
    }
}
impl<I:RegisterIo> FdCan<I> {
    pub fn send(&mut self,id:u16,payload:&[u8])->Result<(),Error> {
        if id>0x7ff || payload.len()>64 {return Err(Error::InvalidFrame)}
        let io=&self.inner.io;let tx=B+0x48;
        if self.inner.status().bus_off {return Err(Error::BusOff)}
        if !io.wait(C+0xcc,1,0) {return Err(Error::Timeout)}
        let dlc=FD_LENGTHS.iter().position(|&n|n>=payload.len()).unwrap();
        let mut padded=[0;64];padded[..payload.len()].copy_from_slice(payload);
        io.write(tx,(id as u32)<<18);io.write(tx+4,((dlc as u32)<<16)|(1<<21)|(1<<20));
        for n in 0..16 {io.write(tx+8+n*4,u32::from_le_bytes(padded[n*4..n*4+4].try_into().unwrap()))}
        io.write(C+0x50,(1<<9)|(1<<27)|(1<<28));io.write(C+0xd0,1);
        for _ in 0..1_000_000 {
            if self.inner.status().bus_off {return Err(Error::BusOff)}
            let ir=io.read(C+0x50);
            if ir&(1<<17)!=0 {return Err(Error::Protocol)}
            if ir&(1<<9)!=0 && io.read(C+0xd8)&1!=0 {return Ok(())}
        }
        io.write(C+0xd4,1);Err(Error::Timeout)
    }
    pub fn receive(&mut self)->Result<Option<FdFrame>,Error> {
        if self.inner.status().bus_off {return Err(Error::BusOff)}
        let io=&self.inner.io;
        if io.read(C+0xa4)&0x7f==0 {return Ok(None)}
        let h=io.read(B);let c=io.read(B+4);
        if h&((1<<29)|(1<<30))!=0 || c&((1<<21)|(1<<20))!=((1<<21)|(1<<20)) {
            io.write(C+0xa8,0);return Err(Error::UnsupportedFrame)
        }
        let len=FD_LENGTHS[((c>>16)&15)as usize];let mut data=[0;64];
        for i in 0..16 {data[i*4..i*4+4].copy_from_slice(&io.read(B+8+i*4).to_le_bytes())}
        io.write(C+0xa8,0);Ok(Some(FdFrame{id:((h>>18)&0x7ff)as u16,len,data}))
    }
    pub fn stop(self) {
        let io=&self.inner.io;io.write(C+0x54,0);io.write(C+0x5c,0);
        io.modify(C+0x18,0,1);let _=io.wait(C+0x18,1,1);io.write(B+0x7908,1);
    }
}
