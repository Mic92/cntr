use files::{Fd, FdState, fd_path};
use fs::POSIX_ACL_DEFAULT_XATTR;
use fsuid;
use fuse::FileType;
use nix;
use nix::fcntl;
use nix::fcntl::OFlag;
use nix::sys::stat;
use parking_lot::RwLock;
use std::ffi::OsStr;
use std::path::Path;
use xattr;

pub struct Inode {
    pub fd: RwLock<Fd>,
    pub kind: FileType,
    pub ino: u64,
    pub dev: u64,
    pub nlookup: RwLock<u64>,
    pub has_default_acl: RwLock<Option<bool>>,
}

impl Inode {
    pub fn upgrade_fd(&self, state: FdState) -> nix::Result<()> {
        let fd = self.fd.upgradable_read();
        if fd.state >= state {
            return Ok(());
        }
        let mut fd = fd.upgrade();

        let perm = if state == FdState::ReadWritable {
            OFlag::O_RDWR
        } else {
            OFlag::O_RDONLY
        };

        let flags = perm | OFlag::O_CLOEXEC | OFlag::O_NONBLOCK;

        let path = fd_path(&fd);
        let new_fd = Fd {
            number: try!(fcntl::open(Path::new(&path), flags, stat::Mode::empty())),
            state: FdState::from(flags),
        };
        *fd = new_fd;

        Ok(())
    }

    pub fn check_default_acl(&self) -> nix::Result<bool> {
        fsuid::set_root();

        let state = self.has_default_acl.upgradable_read();
        if let Some(s) = *state {
            return Ok(s);
        }
        let mut state = state.upgrade();

        try!(self.upgrade_fd(FdState::Readable));
        let fd = self.fd.read();

        let res = xattr::getxattr(
            &fd,
            self.kind,
            &OsStr::new(POSIX_ACL_DEFAULT_XATTR),
            &mut [],
        );
        *state = Some(res.is_ok());
        Ok(res.is_ok())
    }
}
