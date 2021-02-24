use libc::{setfsgid, setfsuid, uid_t};
use std::cell::Cell;

const CURRENT_FSUID: uid_t = (-1_i32) as uid_t;

thread_local! {
    static FSUID : Cell<u32> = Cell::new(unsafe { setfsuid(CURRENT_FSUID) as u32 });
    static FSGID : Cell<u32> = Cell::new(unsafe { setfsgid(CURRENT_FSUID) as u32 });
}

pub fn set_root() {
    set_user_group(0, 0);
}

pub fn set_user_group(uid: u32, gid: u32) {
    // setfsuid/setfsgid set no error on failure

    FSUID.with(|fsuid| {
        if fsuid.get() != uid {
            unsafe { setfsuid(uid) };
        }
        fsuid.set(uid);
    });

    FSGID.with(|fsgid| {
        if fsgid.get() != gid {
            unsafe { setfsgid(gid) };
        }
        fsgid.set(gid);
    });
}
