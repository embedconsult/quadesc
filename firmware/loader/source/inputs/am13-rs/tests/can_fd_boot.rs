use am13_rs::{io::RegisterIo,Peripherals};
use std::{cell::RefCell,collections::BTreeMap,rc::Rc};
#[derive(Clone,Default)]struct Model(Rc<RefCell<BTreeMap<usize,u32>>>);
impl RegisterIo for Model {
 fn read(&self,a:usize)->u32{*self.0.borrow().get(&a).unwrap_or(&0)}
 fn write(&self,a:usize,v:u32){
  // Target readback showed divider writes ignored before request clock enable.
  if a==0x40117904 && self.read(0x40117900)&1==0 {return}
  if a==0x40117050 {let old=self.read(a);self.0.borrow_mut().insert(a,old&!v);return}
  self.0.borrow_mut().insert(a,v);
  if a==0x400b0108 {self.0.borrow_mut().insert(0x400b0204,0x200100|if v&0x100!=0 {0x200}else{0x4000});}
  if a==0x400b0104 && v&(1<<16)!=0 {let old=self.read(0x400b0204);self.0.borrow_mut().insert(0x400b0204,old|16);}
  if a==0x401170d0 {
   let tx=0x40110000+(self.read(0x401170c0)&0xfffc) as usize;
   assert_eq!(tx,0x40110048);assert_eq!(self.read(tx),0x711<<18);
   assert_eq!(self.read(tx+4),(15<<16)|(1<<20)|(1<<21));
   for i in 0..16 {assert_eq!(self.read(tx+8+i*4),u32::from_le_bytes([(4*i)as u8,(4*i+1)as u8,(4*i+2)as u8,(4*i+3)as u8]));}
   self.0.borrow_mut().insert(0x40117050,1<<9);self.0.borrow_mut().insert(0x401170d8,1);
  }
 }
 fn delay_cycles(&self,_:u32){}
}
#[test]fn full_fd_brs_buffers_and_pll_timing(){
 let io=Model::default();io.write(0x400b0204,0x204100);io.write(0x40029000,0x200);io.write(0x40117208,2);
 let mut can=unsafe{Peripherals::from_io(io.clone())}.mcan0.configure_boot_fd().unwrap();
 assert_eq!(io.read(0x400b0120)&0xff31,0x911);assert_eq!(io.read(0x400b0124)&0x7f03,15<<8);
 assert_eq!(io.read(0x400b013c)&0xf000,0x8000);assert_eq!(io.read(0x400b0104)&0x10000,0);assert_eq!(io.read(0x40117904),0);assert_eq!(io.read(0x400b0140)&0x100,0x100);
 let n=io.read(0x4011701c);let d=io.read(0x4011700c);
 assert_eq!(10_000_000/(((n>>16&511)+1)*(1+(n>>8&255)+1+(n&127)+1)),500_000);
 assert_eq!(10_000_000/(((d>>16&31)+1)*(1+(d>>8&31)+1+(d>>4&15)+1)),2_000_000);
 assert_eq!(io.read(0x401170bc),7);assert_eq!(io.read(0x401170c8),7);
 let data:Vec<u8>=(0..64).collect();can.send(0x711,&data).unwrap();
 io.write(0x40110000,0x710<<18);io.write(0x40110004,(15<<16)|(1<<20)|(1<<21));
 for(i,c)in data.chunks(4).enumerate(){io.write(0x40110008+i*4,u32::from_le_bytes(c.try_into().unwrap()));}
 io.write(0x401170a4,1);let f=can.receive().unwrap().unwrap();assert_eq!(f.len,64);assert_eq!(f.data.as_slice(),data);
 io.write(0x40110004,8<<16);assert!(can.receive().is_err());
}
#[test]fn missing_crystal_fails_bounded(){
 let io=Model::default();io.write(0x40117208,2);
 assert!(unsafe{Peripherals::from_io(io)}.mcan0.configure_boot_fd().is_err());
}
