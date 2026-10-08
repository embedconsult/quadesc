// Copyright 2025 Bloxide, all rights reserved
#![no_std]
#[derive(Clone, Copy, Debug)]
pub struct SetLevel {
    pub value: u32,
}
#[derive(Clone, Copy, Debug)]
pub enum LampMsg {
    SetLevel(SetLevel),
}
