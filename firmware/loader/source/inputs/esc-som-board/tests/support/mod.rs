#![allow(dead_code)]
use am13_rs::io::RegisterIo;
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};
#[derive(Default)]
pub struct State {
    pub values: BTreeMap<usize, u32>,
    pub forced: BTreeMap<usize, u32>,
    pub writes: Vec<(usize, u32)>,
    pub reads: Vec<usize>,
}
#[derive(Clone, Default)]
pub struct Model(pub Rc<RefCell<State>>);
impl Model {
    pub fn value(&self, a: usize) -> u32 {
        *self.0.borrow().values.get(&a).unwrap_or(&0)
    }
    pub fn force(&self, a: usize, value: u32) {
        self.0.borrow_mut().forced.insert(a, value);
    }
}
impl RegisterIo for Model {
    fn read(&self, a: usize) -> u32 {
        let mut s = self.0.borrow_mut();
        s.reads.push(a);
        *s.forced.get(&a).or(s.values.get(&a)).unwrap_or(&0)
    }
    fn write(&self, a: usize, value: u32) {
        let mut s = self.0.borrow_mut();
        s.writes.push((a, value));
        s.values.insert(a, value);
        for base in [0x400f0000, 0x400f2000, 0x400f4000] {
            for (offset, register, set) in [
                (0x1290, 0x1280, true),
                (0x12a0, 0x1280, false),
                (0x12d0, 0x12c0, true),
                (0x12e0, 0x12c0, false),
            ] {
                if a == base + offset {
                    let old = *s.values.get(&(base + register)).unwrap_or(&0);
                    let new = if set { old | value } else { old & !value };
                    s.values.insert(base + register, new);
                    if register == 0x1280 {
                        s.values.insert(base + 0x1380, new);
                    }
                }
            }
        }
        if a == 0x40641048 {
            s.values.insert(0x40641030, 0);
        }
    }
    fn delay_cycles(&self, _: u32) {}
}
