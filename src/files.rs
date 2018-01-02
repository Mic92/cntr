use cntr_nix::unistd;
use std::os::unix::prelude::*;

pub struct Fd {
    pub number: RawFd,
    pub is_mutable: bool,
}

impl Fd {
    pub fn raw(&self) -> RawFd {
        self.number
    }
}

impl Drop for Fd {
    fn drop(&mut self) {
        unistd::close(self.number).unwrap();
    }
}
