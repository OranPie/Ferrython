use ferrython_vm::frame::Frame;
use std::mem::size_of;

fn main() {
    println!("Frame struct size: {} bytes", size_of::<Frame>());
}
