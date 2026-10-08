use am13_rs::{can::Frame, io::RegisterIo, Peripherals};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};
#[derive(Clone, Default)]
struct Model(Rc<RefCell<BTreeMap<usize,u32>>>);
impl RegisterIo for Model {
    fn read(&self,a:usize)->u32 { *self.0.borrow().get(&a).unwrap_or(&0) }
    fn write(&self,a:usize,v:u32) {
        if a==0x4011_7050 { self.0.borrow_mut().entry(a).and_modify(|x| *x &= !v); return; }
        self.0.borrow_mut().insert(a,v);
        if a==0x4011_70d0 {
            // Model the controller's real TXBC byte-address fetch, not the driver's TX constant.
            assert_eq!(self.read(0x4011_7908)&1,0,"clock-stop must be disabled");
            assert_eq!(self.read(0x4011_7900)&1,1,"module clock must be enabled");
            let tx=0x4011_0000+(self.read(0x4011_70c0)&0xfffc) as usize;
            assert_eq!(self.read(tx),0x701<<18);
            assert_eq!(self.read(tx+4),3<<16);
            assert_eq!(self.read(tx+8)&0xffffff,0x0578ff);
            if self.read(0x4011_70e0)&1 != 0 { self.0.borrow_mut().insert(0x4011_7050,1<<9); }
            self.0.borrow_mut().insert(0x4011_70d8,1);
        }
    }
    fn delay_cycles(&self,_:u32){}
}
#[test]
fn controller_fetches_xcp_reply_from_configured_ram() {
    let m=Model::default();m.write(0x400b_0204,1<<8);m.write(0x4011_7208,2);
    let dev=unsafe {Peripherals::from_io(m.clone())};
    let mut can=dev.mcan0.configure(500_000).unwrap();
    let bt=m.read(0x4011_701c);
    let brp=((bt>>16)&0x1ff)+1;let tq=1+((bt>>8)&0xff)+1+(bt&0x7f)+1;
    assert_eq!(12_500_000/(brp*tq),500_000);
    assert_eq!(can.receive().unwrap(),None);
    m.write(0x4011_0000,0x700<<18);m.write(0x4011_0004,2<<16);
    m.write(0x4011_0008,0x000000ff);m.write(0x4011_70a4,1);
    let rx=can.receive().unwrap().unwrap();assert_eq!((rx.id,rx.len,rx.data[0]),(0x700,2,0xff));
    can.send(Frame::new(0x701,&[0xff,0x78,0x05]).unwrap()).unwrap();
}
