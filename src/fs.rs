
use fsuid;
use fuse::{self, FileAttr, FileType, Filesystem, ReplyAttr, ReplyXattr, ReplyData, ReplyDirectory,
           ReplyEmpty, ReplyEntry, ReplyOpen, ReplyWrite, ReplyStatfs, ReplyLock, ReplyCreate,
           ReplyIoctl, Request, BackgroundSession};
use ioctl;
use libc::{self, dev_t, c_long};
use nix::{self, unistd, fcntl, dirent};
use nix::sys::{stat, resource};
use nix::sys::time::{TimeSpec as NixTimeSpec, TimeValLike};
use nix::sys::uio::{pread, pwrite};
use statvfs::fstatvfs;
use std::{u32, u64};
use std::collections::HashMap;
use std::ffi::{CStr, OsStr, OsString};
use std::os::unix::prelude::*;
use std::path::Path;
use time::Timespec;
use types::{Error, Result};
use xattr::{fsetxattr, fgetxattr, flistxattr, fremovexattr};

const INODE_MAGIC: char = 'I';
const FH_MAGIC: char = 'F';
const INODE_DELETED_MAGIC: char = 'D';
const DIRP_MAGIC: char = 'D';

struct Fd(RawFd);
impl Fd {
    fn raw(&self) -> RawFd {
        self.0
    }
}

impl Drop for Fd {
    fn drop(&mut self) {
        unistd::close(self.0).unwrap();
    }
}

struct Inode {
    magic: char,
    fd: Fd,
    fd_is_mutable: bool,
    kind: FileType,
    ino: u64,
    dev: u64,
    nlookup: u64,
}

// returns a new file descriptor pointing to the same file
// NOTE: does not work for symlinks
fn reopen_fd(fd: &mut Fd, open_flags: fcntl::OFlag) -> nix::Result<Fd> {
    let path = fd_path(&fd.raw());
    let fd = try!(fcntl::open(
        Path::new(&path),
        open_flags | fcntl::O_CLOEXEC,
        stat::Mode::empty(),
    ));
    return Ok(Fd(fd));
}

impl Inode {
    fn get_mutable_fd(&mut self) -> nix::Result<&Fd> {
        if self.fd_is_mutable {
            return Ok(&self.fd);
        }
        let open_flags = match self.kind {
            FileType::Directory => fcntl::O_RDONLY,
            _ => fcntl::O_RDWR,
        };

        self.fd = try!(reopen_fd(&mut self.fd, open_flags));
        self.fd_is_mutable = true;

        return Ok(&self.fd);
    }
}

#[derive(Hash, Eq, PartialEq)]
struct InodeKey {
    ino: u64,
    dev: u64,
}

struct DirP {
    magic: char,
    dp: dirent::DirectoryStream,
    offset: c_long,
    entry: Option<libc::dirent64>,
}

struct Fh {
    magic: char,
    fd: Fd,
}

impl Fh {
    fn new(fd: Fd) -> Box<Self> {
        Box::new(Fh {
            magic: FH_MAGIC,
            fd: fd,
        })
    }
}

pub struct CntrFs {
    prefix: String,
    root_inode: Box<Inode>,
    inodes: HashMap<InodeKey, Box<Inode>>,
    fuse_fd: RawFd,
}

const TTL: Timespec = Timespec { sec: 1, nsec: 0 };

macro_rules! tryfuse {
    ($result:expr, $reply:expr)  => (match $result {
        Ok(val) => val,
        Err(err) => {
            debug!("return error {} on {}:{}", err, file!(), line!());
            let rc = match err {
                nix::Error::Sys(errno) => errno as i32,
                // InvalidPath, InvalidUtf8, UnsupportedOperation
                _ => libc::EINVAL
            };
            return $reply.error(rc);
        }
    })
}

impl CntrFs {
    pub fn new(prefix: &str) -> Result<CntrFs> {
        let fuse_fd = tryfmt!(
            fcntl::open("/dev/fuse", fcntl::O_RDWR, stat::Mode::empty()),
            "failed to open /dev/fuse"
        );

        let limit = resource::Rlimit {
            rlim_cur: 1048576,
            rlim_max: 1048576,
        };
        tryfmt!(
            resource::setrlimit(resource::Resource::RLIMIT_NOFILE, &limit),
            "Cannot raise file descriptor limit"
        );

        let fd = tryfmt!(
            fcntl::open(
                prefix,
                fcntl::O_RDONLY | fcntl::O_CLOEXEC,
                stat::Mode::all(),
            ),
            "failed to open backing filesystem '{}'",
            prefix
        );
        let name = Path::new(prefix).file_name();
        if name.is_none() {
            return errfmt!(format!(
                "cannot obtain filename of mountpoint: '{}'",
                prefix
            ));
        }
        Ok(CntrFs {
            prefix: String::from(prefix),
            root_inode: Box::new(Inode {
                magic: INODE_MAGIC,
                fd: Fd(fd),
                fd_is_mutable: true,
                kind: FileType::Directory,
                ino: fuse::FUSE_ROOT_ID,
                dev: fuse::FUSE_ROOT_ID,
                nlookup: 2,
            }),
            inodes: HashMap::new(),
            fuse_fd: fuse_fd,
        })
    }

    pub fn mount(self, mountpoint: &Path) -> Result<BackgroundSession> {
        let mount_flags = format!(
            "fd={},rootmode=40000,user_id=0,group_id=0,allow_other",
            self.fuse_fd
        );

        // TODO: allow_other option
        tryfmt!(
            nix::mount::mount(
                Some(self.prefix.as_str()),
                mountpoint,
                Some("fuse.cntr"),
                nix::mount::MsFlags::empty(),
                Some(mount_flags.as_str()),
            ),
            "failed to mount fuse"
        );

        let fuse_fd = self.fuse_fd;
        let session = unsafe {
            BackgroundSession::new(fuse::Session::new_from_fd(self, fuse_fd, mountpoint))
        };
        Ok(tryfmt!(session, "failed to spawn filesystem thread"))
    }

    fn inode(&mut self, ino: u64) -> &mut Inode {
        assert!(ino > 0);
        if ino == fuse::FUSE_ROOT_ID {
            &mut self.root_inode
        } else {
            let inode = unsafe { &mut (*(ino as *mut Inode)) };
            assert!(inode.magic == INODE_MAGIC);
            inode
        }
    }

    fn lookup_from_fd(&mut self, newfd: RawFd) -> nix::Result<FileAttr> {
        let _stat = try!(stat::fstat(newfd));
        let mut attr = attr_from_stat(_stat);

        let key = InodeKey {
            ino: attr.ino,
            dev: _stat.st_dev,
        };

        if self.inodes.contains_key(&key) {
            let inode_ref = self.inodes.get_mut(&key).unwrap();
            inode_ref.as_mut().nlookup += 1;
            let _ = unistd::close(newfd);
            attr.ino = (inode_ref.as_ref() as *const Inode) as u64;
        } else {
            let inode = Box::new(Inode {
                magic: INODE_MAGIC,
                fd: Fd(newfd),
                fd_is_mutable: attr.kind == FileType::Symlink,
                kind: attr.kind,
                ino: attr.ino,
                dev: _stat.st_dev,
                nlookup: 1,
            });
            attr.ino = (inode.as_ref() as *const Inode) as u64;
            self.inodes.insert(key, inode);
        };

        Ok(attr)
    }
}

fn get_filehandle<'a>(fh: u64) -> &'a Fh {
    let handle = unsafe { &mut (*(fh as *mut Fh)) };
    assert!(handle.magic == FH_MAGIC);
    handle
}

fn to_utimespec(time: &Option<Timespec>) -> stat::UtimeSpec {
    time.map_or(stat::UtimeSpec::Omit, |v| {
        let t = NixTimeSpec::seconds(v.sec) + NixTimeSpec::nanoseconds(v.nsec as i64);
        stat::UtimeSpec::Time(t)
    })
}

fn set_time(inode: &Inode, mtime: &Option<Timespec>, atime: &Option<Timespec>) -> nix::Result<()> {
    if inode.kind == FileType::Symlink {
        // FIXME: fs_perms 660 99 99 100 99 t 1 return NOPERM for
        // utime(file) as user 100:99 when file is owned by 99:99
        let path = fd_path(&inode.fd.raw());
        try!(stat::utimensat(
                libc::AT_FDCWD,
                Path::new(&path),
                &to_utimespec(&mtime),
                &to_utimespec(&atime),
                fcntl::AtFlags::empty()));
    } else {
        try!(stat::utimensat(
                inode.fd.raw(),
                "",
                &to_utimespec(&mtime),
                &to_utimespec(&atime),
                fcntl::AtFlags::empty()));
    }

    Ok(())
}

fn fd_path<'a>(fd: &'a RawFd) -> String {
    format!("/proc/self/fd/{}", fd)
}

fn dtype_kind(dtype: u8) -> FileType {
    match dtype {
        libc::DT_BLK => return FileType::BlockDevice,
        libc::DT_CHR => return FileType::CharDevice,
        libc::DT_DIR => return FileType::Directory,
        libc::DT_FIFO => return FileType::NamedPipe,
        libc::DT_LNK => return FileType::Symlink,
        libc::DT_SOCK => return FileType::Socket,
        libc::DT_REG => FileType::RegularFile,
        _ => panic!("BUG! got unknown d_entry type received from d_type"),
    }
}

fn inode_kind(mode: stat::SFlag) -> FileType {
    match mode {
        stat::S_IFBLK => FileType::BlockDevice,
        stat::S_IFCHR => FileType::CharDevice,
        stat::S_IFDIR => FileType::Directory,
        stat::S_IFIFO => FileType::NamedPipe,
        stat::S_IFLNK => FileType::Symlink,
        stat::S_IFREG => FileType::RegularFile,
        stat::S_IFSOCK => FileType::Socket,
        _ => panic!("Got unexpected File type with value: {}", mode.bits()),
    }
}

fn attr_from_stat(attr: stat::FileStat) -> FileAttr {
    let ctime = Timespec::new(attr.st_ctime, attr.st_ctime_nsec as i32);
    FileAttr {
        ino: attr.st_ino, // replaced by ino pointer
        size: attr.st_size,
        blocks: attr.st_blocks as u64,
        atime: Timespec::new(attr.st_atime, attr.st_atime_nsec as i32),
        mtime: Timespec::new(attr.st_mtime, attr.st_mtime_nsec as i32),
        ctime: ctime,
        crtime: ctime,
        uid: attr.st_uid,
        gid: attr.st_gid,
        perm: attr.st_mode as u16,
        kind: inode_kind(stat::SFlag::from_bits_truncate(attr.st_mode & libc::S_IFMT)),
        nlink: attr.st_nlink as u32,
        rdev: attr.st_rdev as u32,
        // Flags (OS X only, see chflags(2))
        flags: 0,
    }
}

pub fn readlinkat<'a>(fd: RawFd) -> nix::Result<OsString> {
    let mut buf = vec![0; (libc::PATH_MAX + 1) as usize];
    loop {
        match fcntl::readlinkat(fd, "", &mut buf) {
            Ok(target) => {
                return Ok(OsString::from(target));
            }
            Err(nix::Error::Sys(nix::Errno::ENAMETOOLONG)) => {}
            Err(e) => return Err(e),
        };
        // Trigger the internal buffer resizing logic of `Vec` by requiring
        // more space than the current capacity. The length is guaranteed to be
        // the same as the capacity due to the if statement above.
        buf.reserve(1)
    }

}

pub fn apply_user_context(req: &Request) {
    fsuid::set_user_group(req.uid(), req.gid());
}

impl Filesystem for CntrFs {
    fn lookup(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEntry) {
        let res = {
            let parent_inode = self.inode(parent);
            fcntl::openat(
                parent_inode.fd.raw(),
                name,
                fcntl::O_PATH | fcntl::O_NOFOLLOW | fcntl::O_CLOEXEC,
                stat::Mode::empty(),
                )
        };
        let newfd = tryfuse!(res, reply);
        let attr = tryfuse!(self.lookup_from_fd(newfd), reply);
        reply.entry(&TTL, &attr, 0);
    }

    fn forget(&mut self, _req: &Request, ino: u64, nlookup: u64) {
        let key = {
            let mut inode = self.inode(ino);
            inode.nlookup -= nlookup;
            if inode.nlookup > 0 {
                return;
            };
            inode.magic = INODE_DELETED_MAGIC;
            &InodeKey {
                ino: inode.ino,
                dev: inode.dev,
            }
        };
        self.inodes.remove(key);
    }

    fn destroy(&mut self, _req: &Request) {
        self.inodes.clear();
    }

    fn getattr(&mut self, req: &Request, ino: u64, reply: ReplyAttr) {
        apply_user_context(req);

        let inode = self.inode(ino);

        let mut attr = attr_from_stat(tryfuse!(stat::fstat(inode.fd.raw()), reply));
        attr.ino = (inode as *const Inode) as u64;
        reply.attr(&TTL, &attr);
    }

    fn setattr(
        &mut self,
        req: &Request,
        ino: u64,
        _mode: Option<u32>,
        uid: Option<u32>,
        gid: Option<u32>,
        _size: Option<i64>,
        atime: Option<Timespec>,
        mtime: Option<Timespec>,
        _fh: Option<u64>,
        _crtime: Option<Timespec>, // only mac os x
        _chgtime: Option<Timespec>, // only mac os x
        _bkuptime: Option<Timespec>, // only mac os x
        _flags: Option<u32>, // only mac os x
        reply: ReplyAttr,
        ) {
        apply_user_context(req);

        let fd = if let Some(fh) = _fh {
            get_filehandle(fh).fd.raw()
        } else {
            tryfuse!(self.inode(ino).get_mutable_fd(), reply).raw()
        };

        if let Some(mode) = _mode {
            let mode = stat::Mode::from_bits_truncate(mode & !libc::S_IFMT);
            tryfuse!(stat::fchmod(fd, mode), reply);
        }

        if uid.is_some() || gid.is_some() {
            let _uid = uid.map(|u| unistd::Uid::from_raw(u));
            let _gid = gid.map(|g| unistd::Gid::from_raw(g));

            tryfuse!(
                unistd::fchownat(fd, "", _uid, _gid, fcntl::AT_EMPTY_PATH),
                reply
                );
        }

        if let Some(size) = _size {
            tryfuse!(unistd::ftruncate(fd, size), reply);
        }
        if mtime.is_some() || atime.is_some() {
            tryfuse!(set_time(self.inode(ino), &mtime, &atime), reply);
        }
        self.getattr(req, ino, reply)
    }

    fn readlink(&mut self, req: &Request, ino: u64, reply: ReplyData) {
        apply_user_context(req);

        let res = readlinkat(self.inode(ino).fd.raw());
        let target = tryfuse!(res, reply);
        reply.data(&target.into_vec());
    }

    fn mknod(
        &mut self,
        req: &Request,
        parent: u64,
        name: &OsStr,
        mode: u32,
        rdev: u32,
        reply: ReplyEntry,
    ) {
        apply_user_context(req);

        let kind = stat::SFlag::from_bits_truncate(mode);
        let perm = stat::Mode::from_bits_truncate(mode);
        tryfuse!(
            stat::mknodat(
                &self.inode(parent).fd.raw(),
                name,
                kind,
                perm,
                rdev as dev_t,
            ),
            reply
        );
        self.lookup(req, parent, name, reply);
    }

    fn mkdir(&mut self, req: &Request, parent: u64, name: &OsStr, mode: u32, reply: ReplyEntry) {
        apply_user_context(req);

        let perm = stat::Mode::from_bits_truncate(mode);
        tryfuse!(
            unistd::mkdirat(self.inode(parent).fd.raw(), name, perm),
            reply
        );
        self.lookup(req, parent, name, reply);
    }

    fn unlink(&mut self, req: &Request, parent: u64, name: &OsStr, reply: ReplyEmpty) {
        apply_user_context(req);

        let res = unistd::unlinkat(self.inode(parent).fd.raw(), name, fcntl::AtFlags::empty());
        tryfuse!(res, reply);
        reply.ok();
    }

    fn rmdir(&mut self, req: &Request, parent: u64, name: &OsStr, reply: ReplyEmpty) {
        apply_user_context(req);

        let res = unistd::unlinkat(self.inode(parent).fd.raw(), name, fcntl::AT_REMOVEDIR);
        tryfuse!(res, reply);
        reply.ok();
    }

    fn symlink(
        &mut self,
        req: &Request,
        parent: u64,
        name: &OsStr,
        link: &Path,
        reply: ReplyEntry,
    ) {
        apply_user_context(req);

        let res = unistd::symlinkat(link, self.inode(parent).fd.raw(), name);
        tryfuse!(res, reply);
        self.lookup(req, parent, name, reply);
    }

    fn rename(
        &mut self,
        req: &Request,
        parent: u64,
        name: &OsStr,
        newparent: u64,
        newname: &OsStr,
        reply: ReplyEmpty,
    ) {
        apply_user_context(req);

        let res = fcntl::renameat(
            self.inode(parent).fd.raw(),
            name,
            self.inode(newparent).fd.raw(),
            newname,
        );
        tryfuse!(res, reply);
        reply.ok();
    }

    fn link(
        &mut self,
        req: &Request,
        ino: u64,
        newparent: u64,
        newname: &OsStr,
        reply: ReplyEntry,
    ) {
        apply_user_context(req);

        let res = unistd::linkat(
            self.inode(ino).fd.raw(),
            "",
            self.inode(newparent).fd.raw(),
            newname,
            fcntl::AT_EMPTY_PATH,
        );
        tryfuse!(res, reply);
        // just do a lookup for simplicity
        self.lookup(req, newparent, newname, reply);
    }

    fn open(&mut self, req: &Request, ino: u64, flags: u32, reply: ReplyOpen) {
        apply_user_context(req);

        let oflags = fcntl::OFlag::from_bits_truncate(flags as i32);
        let path = fd_path(&self.inode(ino).fd.raw());
        let res = tryfuse!(
            fcntl::open(
                Path::new(&path),
                (oflags & !fcntl::O_NOFOLLOW) | fcntl::O_CLOEXEC,
                stat::Mode::empty(),
            ),
            reply
        );
        let fh = Fh::new(Fd(res));
        reply.opened(Box::into_raw(fh) as u64, 0); // freed by close
    }

    fn read(
        &mut self,
        _req: &Request,
        _ino: u64,
        fh: u64,
        offset: i64,
        size: u32,
        reply: ReplyData,
    ) {
        let handle = get_filehandle(fh);

        let mut v = vec![0; size as usize];
        let buf = v.as_mut_slice();
        tryfuse!(pread(handle.fd.raw(), buf, offset), reply);

        reply.data(buf);
    }

    fn write(
        &mut self,
        _req: &Request,
        _ino: u64,
        fh: u64,
        offset: i64,
        data: &[u8],
        _flags: u32,
        reply: ReplyWrite,
    ) {
        let handle = get_filehandle(fh);

        let written = tryfuse!(pwrite(handle.fd.raw(), data, offset), reply);
        reply.written(written as u32);
    }

    fn flush(&mut self, req: &Request, _ino: u64, fh: u64, _lock_owner: u64, reply: ReplyEmpty) {
        apply_user_context(req);

        let handle = get_filehandle(fh);

        let _ = match unistd::dup(handle.fd.raw()) {
            Ok(fd) => {
                tryfuse!(unistd::close(fd), reply);
                reply.ok();
            }
            Err(_) => reply.error(libc::EIO),
        };
    }

    fn release(
        &mut self,
        _req: &Request,
        _ino: u64,
        fh: u64,
        _flags: u32,
        _lock_owner: u64,
        _flush: bool,
        reply: ReplyEmpty,
    ) {
        unsafe { drop(Box::from_raw(fh as *mut Fh)) };
        reply.ok();
    }

    fn fsync(&mut self, _req: &Request, _ino: u64, fh: u64, datasync: bool, reply: ReplyEmpty) {
        let handle = get_filehandle(fh);

        let fd = handle.fd.raw();
        if datasync {
            tryfuse!(unistd::fsync(fd), reply);
        } else {
            tryfuse!(unistd::fdatasync(fd), reply);
        }

        reply.ok();
    }

    fn opendir(&mut self, req: &Request, ino: u64, _flags: u32, reply: ReplyOpen) {
        apply_user_context(req);

        let fd = tryfuse!(self.inode(ino).get_mutable_fd(), reply);
        let path = fd_path(&fd.raw());
        let dp = tryfuse!(dirent::opendir(Path::new(&path)), reply);

        let dirp = Box::new(DirP {
            magic: DIRP_MAGIC,
            dp: dp,
            offset: 0,
            entry: None,
        });
        reply.opened(Box::into_raw(dirp) as u64, 0); // freed by releasedir
    }

    fn readdir(
        &mut self,
        req: &Request,
        _ino: u64,
        fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        apply_user_context(req);

        let dirp = unsafe { &mut (*(fh as *mut DirP)) };
        assert!(dirp.magic == DIRP_MAGIC);

        if offset != dirp.offset {
            dirent::seekdir(&mut dirp.dp, offset);
            dirp.entry = None;
            dirp.offset = 0;
        }

        while {
            if dirp.entry.is_none() {
                dirp.entry = tryfuse!(dirent::readdir(&mut dirp.dp), reply).map(|v| *v.as_ref());
            }
            match dirp.entry {
                None => false,
                Some(entry) => {
                    dirp.offset = dirent::telldir(&mut dirp.dp);
                    let name = unsafe { CStr::from_ptr(entry.d_name.as_ptr()) };
                    dirp.entry = None;
                    reply.add(
                        entry.d_ino,
                        dirp.offset,
                        dtype_kind(entry.d_type),
                        OsStr::from_bytes(name.to_bytes()),
                    )
                }
            }
        }
        {}
        reply.ok()
    }

    fn releasedir(&mut self, _req: &Request, _ino: u64, fh: u64, _flags: u32, reply: ReplyEmpty) {
        let dirp = unsafe { Box::from_raw(fh as *mut DirP) };
        assert!(dirp.magic == DIRP_MAGIC);
        // dirp out-of-scope -> closedir(dirp.dp)
        reply.ok();
    }

    fn fsyncdir(&mut self, _req: &Request, _ino: u64, fh: u64, datasync: bool, reply: ReplyEmpty) {
        let dirp = unsafe { &mut (*(fh as *mut DirP)) };
        assert!(dirp.magic == DIRP_MAGIC);
        let fd = tryfuse!(dirent::dirfd(&mut dirp.dp), reply);
        if datasync {
            tryfuse!(unistd::fsync(fd), reply);
        } else {
            tryfuse!(unistd::fdatasync(fd), reply);
        }
        reply.ok();
    }

    fn statfs(&mut self, req: &Request, ino: u64, reply: ReplyStatfs) {
        apply_user_context(req);

        let fd = tryfuse!(self.inode(ino).get_mutable_fd(), reply).raw();
        let stat = tryfuse!(fstatvfs(fd), reply);
        reply.statfs(
            stat.f_blocks,
            stat.f_bfree,
            stat.f_bavail,
            stat.f_files,
            stat.f_ffree,
            stat.f_bsize as u32,
            stat.f_namemax as u32,
            stat.f_frsize as u32,
        );
    }

    fn getxattr(&mut self, req: &Request, ino: u64, name: &OsStr, size: u32, reply: ReplyXattr) {
        apply_user_context(req);

        let fd = tryfuse!(self.inode(ino).get_mutable_fd(), reply).raw();

        if size == 0 {
            let size = tryfuse!(fgetxattr(fd, name, &mut []), reply);
            reply.size(size as u32);
        } else {
            let mut buf = vec![0; size as usize];
            let size = tryfuse!(fgetxattr(fd, name, buf.as_mut_slice()), reply);
            reply.data(&buf[..size]);
        }
    }

    fn listxattr(&mut self, req: &Request, ino: u64, size: u32, reply: ReplyXattr) {
        apply_user_context(req);

        let fd = tryfuse!(self.inode(ino).get_mutable_fd(), reply).raw();

        if size == 0 {
            let res = flistxattr(fd, &mut []);
            let size = tryfuse!(res, reply);
            reply.size(size as u32);
        } else {
            let mut buf = vec![0; size as usize];
            let size = tryfuse!(flistxattr(fd, buf.as_mut_slice()), reply);
            reply.data(&buf[..size]);
        }
    }

    fn setxattr(
        &mut self,
        req: &Request,
        ino: u64,
        name: &OsStr,
        value: &[u8],
        flags: u32,
        _position: u32,
        reply: ReplyEmpty,
    ) {
        apply_user_context(req);

        let fd = tryfuse!(self.inode(ino).get_mutable_fd(), reply).raw();
        tryfuse!(fsetxattr(fd, name, value, flags as i32), reply);
        reply.ok();
    }

    fn removexattr(&mut self, req: &Request, ino: u64, name: &OsStr, reply: ReplyEmpty) {
        apply_user_context(req);

        let fd = tryfuse!(self.inode(ino).get_mutable_fd(), reply).raw();
        tryfuse!(fremovexattr(fd, name), reply);
        reply.ok();
    }

    fn access(&mut self, req: &Request, ino: u64, mask: u32, reply: ReplyEmpty) {
        apply_user_context(req);

        let fd = self.inode(ino).fd.raw();
        let mode = unistd::AccessMode::from_bits_truncate(mask as i32);
        tryfuse!(unistd::access(fd_path(&fd).as_str(), mode), reply);
        reply.ok();
    }

    fn create(
        &mut self,
        req: &Request,
        parent: u64,
        name: &OsStr,
        mode: u32,
        flags: u32,
        reply: ReplyCreate,
    ) {
        apply_user_context(req);

        let parent_fd = self.inode(parent).fd.raw();

        let oflag = fcntl::OFlag::from_bits_truncate(flags as i32);
        let create_mode = stat::Mode::from_bits_truncate(mode);
        let fd = tryfuse!(
            fcntl::openat(
                parent_fd,
                name,
                oflag | fcntl::O_NOFOLLOW | fcntl::O_CLOEXEC,
                create_mode,
            ),
            reply
        );
        let fh = Fh::new(Fd(fd));

        let newfd = tryfuse!(unistd::dup(fd), reply);
        let attr = tryfuse!(self.lookup_from_fd(newfd), reply);

        let fp = Box::into_raw(fh) as u64; // freed by close
        reply.created(&TTL, &attr, 0, fp, flags);
    }

    fn getlk(
        &mut self,
        req: &Request,
        _ino: u64,
        fh: u64,
        _lock_owner: u64,
        start: u64,
        end: u64,
        typ: u32,
        pid: u32,
        reply: ReplyLock,
    ) {
        apply_user_context(req);

        let handle = get_filehandle(fh);
        let mut flock = libc::flock {
            l_type: typ as i16,
            l_whence: 0,
            l_start: start as i64,
            l_len: (end - start) as i64,
            l_pid: pid as i32,
        };
        tryfuse!(
            fcntl::fcntl(handle.fd.raw(), fcntl::F_GETLK(&mut flock)),
            reply
        );
        reply.locked(
            flock.l_start as u64,
            (flock.l_start + flock.l_len) as u64,
            flock.l_type as u32,
            flock.l_pid as u32,
        )
    }

    fn setlk(
        &mut self,
        req: &Request,
        _ino: u64,
        fh: u64,
        _lock_owner: u64,
        start: u64,
        end: u64,
        typ: u32,
        pid: u32,
        _sleep: bool,
        reply: ReplyEmpty,
    ) {
        apply_user_context(req);

        let handle = get_filehandle(fh);
        let mut flock = libc::flock {
            l_type: typ as i16,
            l_whence: 0,
            l_start: start as i64,
            l_len: (end - start) as i64,
            l_pid: pid as i32,
        };
        tryfuse!(
            fcntl::fcntl(handle.fd.raw(), fcntl::F_SETLK(&mut flock)),
            reply
        );
        reply.ok()
    }

    /// Preallocate or deallocate space to a file
    fn fallocate(
        &mut self,
        _req: &Request,
        _ino: u64,
        fh: u64,
        offset: i64,
        length: i64,
        mode: i32,
        reply: ReplyEmpty,
    ) {
        let handle = get_filehandle(fh);
        let flags = fcntl::FallocateFlags::from_bits_truncate(mode);
        tryfuse!(
            fcntl::fallocate(handle.fd.raw(), flags, offset, length),
            reply
        );
        reply.ok();
    }

    fn ioctl(
        &mut self,
        req: &Request,
        _ino: u64,
        fh: u64,
        flags: u32,
        _cmd: u32,
        in_data: Option<&[u8]>,
        out_size: u32,
        reply: ReplyIoctl,
    ) {
        apply_user_context(req);

        let fd = if (flags & fuse::consts::FUSE_IOCTL_DIR) > 0 {
            let dirp = unsafe { &mut (*(fh as *mut DirP)) };
            assert!(dirp.magic == DIRP_MAGIC);
            tryfuse!(dirent::dirfd(&mut dirp.dp), reply)
        } else {
            get_filehandle(fh).fd.raw()
        };

        let cmd = _cmd as libc::c_ulong;

        if out_size > 0 {
            let mut out = vec![0; out_size as usize];
            if let Some(data) = in_data {
                out[..data.len()].clone_from_slice(data);
            }
            tryfuse!(ioctl::ioctl_read(fd, cmd, out.as_mut_slice()), reply);
            reply.ioctl(0, out.as_slice());
        } else if let Some(data) = in_data {
            tryfuse!(ioctl::ioctl_write(fd, cmd, data), reply);
            reply.ioctl(0, &[]);
        } else {
            tryfuse!(ioctl::ioctl(fd, cmd), reply);
            reply.ioctl(0, &[]);
        }
    }
}
