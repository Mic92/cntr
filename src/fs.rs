use concurrent_hashmap::ConcHashMap;
use files::{Fd, FdState, fd_path};
use fsuid;
use fuse::{self, FileAttr, FileType, Filesystem, ReplyAttr, ReplyXattr, ReplyData, ReplyEmpty,
           ReplyEntry, ReplyOpen, ReplyWrite, ReplyStatfs, ReplyCreate, ReplyIoctl, Request,
           ReplyLseek, ReplyRead};
use fusefd;
use inode::Inode;
use ioctl;
use libc::{self, dev_t, c_long};
use nix::{self, unistd, dirent};
use nix::errno::Errno;
use nix::fcntl::{self, SpliceFFlags, OFlag, AtFlags};
use nix::sys::{stat, resource};
use nix::sys::stat::SFlag;
use nix::sys::time::{TimeSpec as NixTimeSpec, TimeValLike};
use nix::sys::uio::{pread, pwrite};
use nix::unistd::{Gid, Uid};
use num_cpus;
use parking_lot::{RwLock, Mutex};
use readlink::readlinkat;
use statvfs::fstatvfs;
use std::{u32, u64};
use std::cmp;
use std::collections::HashMap;
use std::ffi::{CStr, OsStr};
use std::fs::File;
use std::io;
use std::mem;
use std::os::unix::prelude::*;
use std::path::Path;
use std::sync::Arc;
use std::vec::Vec;
use thread_scoped::{scoped, JoinGuard};
use time::Timespec;
use types::{Error, Result};
use user_namespace::IdMap;
use xattr;

const FH_MAGIC: char = 'F';
const DIRP_MAGIC: char = 'D';
pub const POSIX_ACL_DEFAULT_XATTR: &str = "system.posix_acl_default";

#[derive(Hash, Eq, PartialEq, Clone, Debug)]
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

struct InodeCounter {
    next_number: u64,
    generation: u64,
}

pub struct CntrFs {
    prefix: String,
    root_inode: Arc<Inode>,
    inode_mapping: Arc<Mutex<HashMap<InodeKey, u64>>>,
    inodes: Arc<ConcHashMap<u64, Arc<Inode>>>,
    inode_counter: Arc<RwLock<InodeCounter>>,
    effective_uid: Option<Uid>,
    effective_gid: Option<Gid>,
    fuse_fd: RawFd,
    uid_map: IdMap,
    gid_map: IdMap,
    splice_read: bool,
    splice_write: bool,
}

enum ReplyDirectory {
    Directory(fuse::ReplyDirectory),
    DirectoryPlus(fuse::ReplyDirectoryPlus),
}

impl ReplyDirectory {
    pub fn ok(self) {
        match self {
            ReplyDirectory::Directory(r) => r.ok(),
            ReplyDirectory::DirectoryPlus(r) => r.ok(),
        }
    }

    pub fn error(self, err: libc::c_int) {
        match self {
            ReplyDirectory::Directory(r) => r.error(err),
            ReplyDirectory::DirectoryPlus(r) => r.error(err),
        }
    }
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

// TODO: evaluate if this option increases performance
fn posix_fadvise(fd: RawFd) -> nix::Result<()> {
    let res = unsafe { libc::posix_fadvise(fd, 0, 0, libc::POSIX_FADV_NOREUSE) };
    Errno::result(res).map(drop)
}

pub struct CntrMountOptions<'a> {
    pub prefix: &'a str,
    pub splice_read: bool,
    pub splice_write: bool,
    pub uid_map: IdMap,
    pub gid_map: IdMap,
    pub effective_uid: Option<Uid>,
    pub effective_gid: Option<Gid>,
}

pub enum LookupFile<'a> {
    Donate(File),
    Borrow(&'a File),
}

impl<'a> AsRawFd for LookupFile<'a> {
    fn as_raw_fd(&self) -> RawFd {
        match self {
            &LookupFile::Donate(ref f) => f.as_raw_fd(),
            &LookupFile::Borrow(ref f) => f.as_raw_fd(),
        }
    }
}

impl<'a> LookupFile<'a> {
    fn into_raw_fd(self) -> nix::Result<RawFd> {
        match self {
            LookupFile::Donate(f) => Ok(f.into_raw_fd()),
            LookupFile::Borrow(f) => unistd::dup(f.as_raw_fd()),
        }
    }
}

impl CntrFs {
    pub fn new(options: &CntrMountOptions) -> Result<CntrFs> {
        let fuse_fd = tryfmt!(fusefd::open(), "failed to initialize fuse");

        let limit = resource::Rlimit {
            rlim_cur: 1_048_576,
            rlim_max: 1_048_576,
        };
        tryfmt!(
            resource::setrlimit(resource::Resource::RLIMIT_NOFILE, &limit),
            "Cannot raise file descriptor limit"
        );

        let fd = tryfmt!(
            fcntl::open(
                options.prefix,
                OFlag::O_RDONLY | OFlag::O_CLOEXEC,
                stat::Mode::all(),
            ),
            "failed to open backing filesystem '{}'",
            options.prefix
        );
        Ok(CntrFs {
            prefix: String::from(options.prefix),
            root_inode: Arc::new(Inode {
                fd: RwLock::new(Fd::new(fd, FdState::Readable)),
                kind: FileType::Directory,
                ino: fuse::FUSE_ROOT_ID,
                dev: fuse::FUSE_ROOT_ID,
                nlookup: RwLock::new(2),
                has_default_acl: RwLock::new(None),
            }),
            inode_mapping: Arc::new(Mutex::new(HashMap::<InodeKey, u64>::new())),
            inodes: Arc::new(ConcHashMap::<u64, Arc<Inode>>::new()),
            inode_counter: Arc::new(RwLock::new(InodeCounter {
                next_number: 3,
                generation: 0,
            })),
            uid_map: options.uid_map,
            gid_map: options.gid_map,
            fuse_fd: fuse_fd.into_raw_fd(),
            splice_read: options.splice_read,
            splice_write: options.splice_write,
            effective_uid: options.effective_uid,
            effective_gid: options.effective_gid,
        })
    }

    pub fn uid_map(&self) -> IdMap {
        self.uid_map
    }

    pub fn gid_map(&self) -> IdMap {
        self.gid_map
    }

    fn create_file(
        &self,
        req: &Request,
        parent: u64,
        name: &OsStr,
        mut mode: u32,
        umask: u32,
        flags: u32,
    ) -> nix::Result<RawFd> {
        let parent_inode = try!(self.inode(&parent));
        let has_default_acl = try!(parent_inode.check_default_acl());
        let parent_fd = parent_inode.fd.read();

        self.set_user_group(req);

        let oflag = fcntl::OFlag::from_bits_truncate(flags as i32);

        if !has_default_acl {
            mode &= !umask;
        }

        let create_mode = stat::Mode::from_bits_truncate(mode);
        let fd = try!(fcntl::openat(
            parent_fd.raw(),
            name,
            oflag | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
            create_mode,
        ));
        Ok(fd)
    }

    pub fn spawn_sessions<'a>(self) -> Result<Vec<JoinGuard<'a, io::Result<()>>>> {
        let mut sessions = Vec::new();

        // numbers of sessions is optimized for cached read
        let num_sessions = cmp::max(num_cpus::get() / 2, 1) as usize;

        for _ in 0..num_sessions {
            debug!("spawn worker");
            let cntrfs = CntrFs {
                prefix: self.prefix.clone(),
                root_inode: Arc::clone(&self.root_inode),
                fuse_fd: self.fuse_fd,
                inode_mapping: Arc::clone(&self.inode_mapping),
                inodes: Arc::clone(&self.inodes),
                splice_read: self.splice_read,
                splice_write: self.splice_write,
                inode_counter: Arc::clone(&self.inode_counter),
                uid_map: self.uid_map,
                gid_map: self.gid_map,
                effective_uid: self.effective_uid,
                effective_gid: self.effective_gid,
            };
            let res =
                fuse::Session::new_from_fd(cntrfs, self.fuse_fd, Path::new(""), self.splice_write);
            let session = tryfmt!(res, "failed to inherit fuse session");

            let guard = unsafe {
                scoped(move || {
                    let mut se = session;
                    se.run()
                })
            };

            sessions.push(guard);
        }

        Ok(sessions)
    }

    pub fn mount(&self, mountpoint: &Path) -> Result<()> {
        let mount_flags = format!(
            "fd={},rootmode=40000,user_id=0,group_id=0,allow_other,default_permissions",
            self.fuse_fd
        );

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
        Ok(())
    }

    fn setattr_inner(
        &mut self,
        ino: u64,
        fd: &Fd,
        mode: Option<u32>,
        uid: Option<u32>,
        gid: Option<u32>,
        size: Option<i64>,
        atime: fuse::UtimeSpec,
        mtime: fuse::UtimeSpec,
    ) -> nix::Result<()> {

        if let Some(bits) = mode {
            let mode = stat::Mode::from_bits_truncate(bits);
            try!(stat::fchmod(fd.raw(), mode));
        }

        if uid.is_some() || gid.is_some() {
            let _uid = uid.map(|u| Uid::from_raw(self.uid_map.map_id_up(u)));
            let _gid = gid.map(|g| Gid::from_raw(self.gid_map.map_id_up(g)));

            try!(unistd::fchownat(
                fd.raw(),
                "",
                _uid,
                _gid,
                AtFlags::AT_EMPTY_PATH,
            ));
        }

        if let Some(s) = size {
            try!(unistd::ftruncate(fd.raw(), s));
        }
        if mtime != fuse::UtimeSpec::Omit || atime != fuse::UtimeSpec::Omit {
            let inode = try!(self.inode(&ino));
            try!(set_time(&inode, fd, &mtime, &atime));
        }
        Ok(())
    }


    fn generic_readdir(&mut self, ino: u64, fh: u64, offset: i64, mut reply: ReplyDirectory) {
        fsuid::set_root();

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
                    match reply {
                        ReplyDirectory::Directory(ref mut r) => {
                            r.add(
                                entry.d_ino,
                                dirp.offset,
                                dtype_kind(entry.d_type),
                                OsStr::from_bytes(name.to_bytes()),
                            )
                        }
                        ReplyDirectory::DirectoryPlus(ref mut r) => {
                            match self.lookup_inode(ino, OsStr::from_bytes(name.to_bytes())) {
                                Ok((attr, generation)) => {
                                    r.add(
                                        entry.d_ino,
                                        dirp.offset,
                                        OsStr::from_bytes(name.to_bytes()),
                                        &TTL,
                                        &attr,
                                        generation,
                                    )
                                }
                                _ => true,
                            }
                        }
                    }
                }
            }
        }
        {}
        reply.ok()
    }

    pub fn set_user_group(&self, req: &Request) {
        let real_uid = self.uid_map.map_id_up(req.uid());
        let uid = self.effective_uid.map_or(real_uid, |u| u.into());

        let real_gid = self.gid_map.map_id_up(req.gid());
        let gid = self.effective_gid.map_or(real_gid, |g| g.into());
        fsuid::set_user_group(uid, gid);
    }

    fn attr_from_stat(&self, attr: stat::FileStat) -> FileAttr {
        let ctime = Timespec::new(attr.st_ctime, attr.st_ctime_nsec as i32);
        FileAttr {
            ino: attr.st_ino, // replaced by ino pointer
            size: attr.st_size,
            blocks: attr.st_blocks as u64,
            atime: Timespec::new(attr.st_atime, attr.st_atime_nsec as i32),
            mtime: Timespec::new(attr.st_mtime, attr.st_mtime_nsec as i32),
            ctime: ctime,
            crtime: ctime,
            uid: self.uid_map.map_id_down(attr.st_uid),
            gid: self.gid_map.map_id_down(attr.st_gid),
            perm: attr.st_mode as u16,
            kind: inode_kind(stat::SFlag::from_bits_truncate(attr.st_mode)),
            nlink: attr.st_nlink as u32,
            rdev: attr.st_rdev as u32,
            // Flags (OS X only, see chflags(2))
            flags: 0,
        }
    }

    fn inode<'a>(&'a self, ino: &u64) -> nix::Result<Arc<Inode>> {
        assert!(*ino > 0);

        if *ino == fuse::FUSE_ROOT_ID {
            Ok(Arc::clone(&self.root_inode))
        } else {
            match self.inodes.find(ino) {
                Some(inode) => Ok(Arc::clone(inode.get())),
                None => Err(nix::Error::Sys(Errno::ESTALE)),
            }
        }
    }

    fn mutable_inode(&mut self, ino: &mut u64) -> nix::Result<Arc<Inode>> {

        let inode = try!(self.inode(ino));
        try!(inode.upgrade_fd(FdState::Readable));
        Ok(inode)
    }

    fn next_inode_number(&self) -> (u64, u64) {
        let mut counter = self.inode_counter.write();
        let next_number = counter.next_number;
        counter.next_number += 1;

        if next_number == 0 {
            counter.next_number = fuse::FUSE_ROOT_ID + 1;
            counter.generation += 1;
        }

        (next_number, counter.generation)
    }

    fn lookup_from_fd<'a>(&mut self, new_file: LookupFile<'a>) -> nix::Result<(FileAttr, u64)> {
        let _stat = try!(stat::fstat(new_file.as_raw_fd()));
        let mut attr = self.attr_from_stat(_stat);

        let key = InodeKey {
            ino: attr.ino,
            dev: _stat.st_dev,
        };

        let mut inode_mapping = self.inode_mapping.lock();

        if let Some(ino) = inode_mapping.get(&key) {
            if let Some(mut inode) = self.inodes.find_mut(ino) {
                *inode.get().nlookup.write() += 1;
                let counter = self.inode_counter.read();
                attr.ino = *ino;
                return Ok((attr, counter.generation));
            } else {
                panic!("BUG! could not find inode {} also its mapping exists.", ino);
            };
        }

        let (next_number, generation) = self.next_inode_number();
        let fd = RwLock::new(Fd::new(
            try!(new_file.into_raw_fd()),
            if attr.kind == FileType::Symlink ||
                attr.kind == FileType::BlockDevice
            {
                // we cannot open a symlink read/writable
                FdState::Readable
            } else {
                FdState::None
            },
        ));

        let inode = Arc::new(Inode {
            fd: fd,
            kind: attr.kind,
            ino: attr.ino,
            dev: _stat.st_dev,
            nlookup: RwLock::new(1),
            has_default_acl: RwLock::new(None),
        });
        assert!(self.inodes.insert(next_number, inode).is_none());
        attr.ino = next_number;

        inode_mapping.insert(key, next_number);

        return Ok((attr, generation));
    }

    pub fn lookup_inode(&mut self, parent: u64, name: &OsStr) -> nix::Result<(FileAttr, u64)> {
        fsuid::set_root();
        let res = {
            let parent_inode = try!(self.inode(&parent));
            let parent_fd = parent_inode.fd.read();
            try!(fcntl::openat(
                parent_fd.raw(),
                name,
                OFlag::O_PATH | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
                stat::Mode::empty(),
            ))
        };

        let file = unsafe { File::from_raw_fd(res) };

        self.lookup_from_fd(LookupFile::Donate(file))
    }
}

fn get_filehandle<'a>(fh: u64) -> &'a Fh {
    let handle = unsafe { &mut (*(fh as *mut Fh)) };
    assert!(handle.magic == FH_MAGIC);
    handle
}

fn to_utimespec(time: &fuse::UtimeSpec) -> stat::UtimeSpec {
    match *time {
        fuse::UtimeSpec::Omit => stat::UtimeSpec::Omit,
        fuse::UtimeSpec::Now => stat::UtimeSpec::Now,
        fuse::UtimeSpec::Time(time) => {
            let t = NixTimeSpec::seconds(time.sec) + NixTimeSpec::nanoseconds(i64::from(time.nsec));
            stat::UtimeSpec::Time(t)
        }
    }
}

fn set_time(
    inode: &Inode,
    fd: &Fd,
    mtime: &fuse::UtimeSpec,
    atime: &fuse::UtimeSpec,
) -> nix::Result<()> {
    if inode.kind == FileType::Symlink {
        // FIXME: fs_perms 660 99 99 100 99 t 1 return NOPERM for
        // utime(file) as user 100:99 when file is owned by 99:99
        let path = fd_path(fd);
        try!(stat::utimensat(
            libc::AT_FDCWD,
            Path::new(&path),
            &to_utimespec(mtime),
            &to_utimespec(atime),
            fcntl::AtFlags::empty(),
        ));
    } else {
        try!(stat::futimens(
            fd.raw(),
            &to_utimespec(mtime),
            &to_utimespec(atime),
        ));
    }

    Ok(())
}

fn dtype_kind(dtype: u8) -> FileType {
    match dtype {
        libc::DT_BLK => FileType::BlockDevice,
        libc::DT_CHR => FileType::CharDevice,
        libc::DT_DIR => FileType::Directory,
        libc::DT_FIFO => FileType::NamedPipe,
        libc::DT_LNK => FileType::Symlink,
        libc::DT_SOCK => FileType::Socket,
        libc::DT_REG => FileType::RegularFile,
        _ => panic!("BUG! got unknown d_entry type received from d_type"),
    }
}

fn inode_kind(mode: SFlag) -> FileType {
    match mode {
        SFlag::S_IFBLK => FileType::BlockDevice,
        SFlag::S_IFCHR => FileType::CharDevice,
        SFlag::S_IFDIR => FileType::Directory,
        SFlag::S_IFIFO => FileType::NamedPipe,
        SFlag::S_IFLNK => FileType::Symlink,
        SFlag::S_IFREG => FileType::RegularFile,
        SFlag::S_IFSOCK => FileType::Socket,
        _ => panic!("Got unexpected File type with value: {}", mode.bits()),
    }
}

impl Filesystem for CntrFs {
    fn lookup(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEntry) {
        fsuid::set_root();

        let (attr, generation) = tryfuse!(self.lookup_inode(parent, name), reply);
        reply.entry(&TTL, &attr, generation);
    }
    fn forget(&mut self, _req: &Request, ino: u64, nlookup: u64) {
        fsuid::set_root();

        let mut inode_mapping = self.inode_mapping.lock();

        let key = match self.inodes.find_mut(&ino) {
            Some(ref mut inode_lock) => {
                let inode = inode_lock.get();
                let mut old_nlookup = inode.nlookup.write();
                assert!(*old_nlookup >= nlookup);

                *old_nlookup -= nlookup;

                if *old_nlookup != 0 {
                    return;
                };

                InodeKey {
                    ino: inode.ino,
                    dev: inode.dev,
                }
            }
            None => return,
        };

        self.inodes.remove(&ino);
        inode_mapping.remove(&key);
    }

    fn destroy(&mut self, _req: &Request) {
        fsuid::set_root();
        self.inodes.clear();
    }

    fn getattr(&mut self, _req: &Request, ino: u64, reply: ReplyAttr) {
        fsuid::set_root();

        let inode = tryfuse!(self.inode(&ino), reply);
        let fd = inode.fd.read();

        let mut attr = self.attr_from_stat(tryfuse!(stat::fstat(fd.raw()), reply));
        attr.ino = ino;
        reply.attr(&TTL, &attr);
    }

    fn setattr(
        &mut self,
        _req: &Request,
        ino: u64,
        mode: Option<u32>,
        uid: Option<u32>,
        gid: Option<u32>,
        size: Option<i64>,
        atime: fuse::UtimeSpec,
        mtime: fuse::UtimeSpec,
        fh: Option<u64>,
        _crtime: Option<Timespec>, // only mac os x
        _chgtime: Option<Timespec>, // only mac os x
        _bkuptime: Option<Timespec>, // only mac os x
        _flags: Option<u32>, // only mac os x
        reply: ReplyAttr,
    ) {
        fsuid::set_root();

        {
            if let Some(pointer) = fh {
                let fd = &get_filehandle(pointer).fd;

                tryfuse!(
                    self.setattr_inner(ino, fd, mode, uid, gid, size, atime, mtime),
                    reply
                );
            } else {
                let inode = tryfuse!(self.inode(&ino), reply);
                let state = if size.is_some() {
                    FdState::ReadWritable
                } else {
                    FdState::Readable
                };
                tryfuse!(inode.upgrade_fd(state), reply);
                let fd = inode.fd.read();

                tryfuse!(
                    self.setattr_inner(ino, &fd, mode, uid, gid, size, atime, mtime),
                    reply
                );
            };
        }

        self.getattr(_req, ino, reply)
    }

    fn readlink(&mut self, _req: &Request, ino: u64, reply: ReplyData) {
        fsuid::set_root();

        let inode = tryfuse!(self.inode(&ino), reply);
        let fd = inode.fd.read();
        let target = tryfuse!(readlinkat(fd.raw()), reply);
        reply.data(&target.into_vec());
    }

    fn mknod(
        &mut self,
        req: &Request,
        parent: u64,
        name: &OsStr,
        mut mode: u32,
        umask: u32,
        rdev: u32,
        reply: ReplyEntry,
    ) {
        {
            let inode = tryfuse!(self.inode(&parent), reply);
            let has_default_acl = tryfuse!(inode.check_default_acl(), reply);
            if !has_default_acl {
                mode &= !umask;
            }
            self.set_user_group(req);

            let kind = stat::SFlag::from_bits_truncate(mode);
            let perm = stat::Mode::from_bits_truncate(mode);

            let fd = inode.fd.read();
            tryfuse!(
                stat::mknodat(&fd.raw(), name, kind, perm, dev_t::from(rdev)),
                reply
            );
        }
        self.lookup(req, parent, name, reply);
    }

    fn mkdir(
        &mut self,
        req: &Request,
        parent: u64,
        name: &OsStr,
        mut mode: u32,
        umask: u32,
        reply: ReplyEntry,
    ) {
        {
            let inode = tryfuse!(self.inode(&parent), reply);
            let has_default_acl = tryfuse!(inode.check_default_acl(), reply);
            if !has_default_acl {
                mode &= !umask;
            }
            self.set_user_group(req);

            let perm = stat::Mode::from_bits_truncate(mode);
            let fd = inode.fd.read();
            tryfuse!(unistd::mkdirat(fd.raw(), name, perm), reply);
        }
        self.lookup(req, parent, name, reply);
    }

    fn unlink(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEmpty) {
        fsuid::set_root();

        let inode = tryfuse!(self.inode(&parent), reply);
        let fd = inode.fd.read();

        let res = unistd::unlinkat(fd.raw(), name, fcntl::AtFlags::empty());
        tryfuse!(res, reply);
        reply.ok();
    }

    fn rmdir(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEmpty) {
        fsuid::set_root();

        let inode = tryfuse!(self.inode(&parent), reply);
        let fd = inode.fd.read();

        tryfuse!(
            unistd::unlinkat(fd.raw(), name, AtFlags::AT_REMOVEDIR),
            reply
        );
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
        self.set_user_group(req);

        {
            let inode = tryfuse!(self.inode(&parent), reply);
            let fd = inode.fd.read();
            let res = unistd::symlinkat(link, fd.raw(), name);
            tryfuse!(res, reply);
        }
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
        self.set_user_group(req);

        let parent_inode = tryfuse!(self.inode(&parent), reply);
        let parent_fd = parent_inode.fd.read();
        let new_inode = tryfuse!(self.inode(&newparent), reply);
        let new_fd = new_inode.fd.read();
        tryfuse!(
            fcntl::renameat(parent_fd.raw(), name, new_fd.raw(), newname),
            reply
        );

        reply.ok();
    }

    fn rename2(
        &mut self,
        req: &Request,
        parent: u64,
        name: &OsStr,
        newparent: u64,
        newname: &OsStr,
        flags: u32,
        reply: ReplyEmpty,
    ) {
        self.set_user_group(req);

        let parent_inode = tryfuse!(self.inode(&parent), reply);
        let parent_fd = parent_inode.fd.read();
        let new_inode = tryfuse!(self.inode(&newparent), reply);
        let new_fd = new_inode.fd.read();
        let res = fcntl::renameat2(
            parent_fd.raw(),
            name,
            new_fd.raw(),
            newname,
            fcntl::RenameAt2Flags::from_bits_truncate(flags as i32),
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
        fsuid::set_root();

        {
            let source_inode = tryfuse!(self.inode(&ino), reply);
            let source_fd = source_inode.fd.read();
            let newparent_inode = tryfuse!(self.inode(&newparent), reply);
            let newparent_fd = newparent_inode.fd.read();

            let res = unistd::linkat(
                source_fd.raw(),
                "",
                newparent_fd.raw(),
                newname,
                AtFlags::AT_EMPTY_PATH,
            );
            tryfuse!(res, reply);
        }
        // just do a lookup for simplicity
        self.lookup(req, newparent, newname, reply);
    }

    fn open(&mut self, _req: &Request, ino: u64, flags: u32, reply: ReplyOpen) {
        fsuid::set_root();

        let mut oflags = fcntl::OFlag::from_bits_truncate(flags as i32);
        let inode = tryfuse!(self.inode(&ino), reply);
        let fd = inode.fd.read();
        let path = fd_path(&fd);

        // ignore write only or append flags because we have writeback cache enabled
        // and the kernel will also read from file descriptors opened as read.
        oflags = (oflags & !OFlag::O_NOFOLLOW & !OFlag::O_APPEND) | OFlag::O_CLOEXEC;
        if oflags & OFlag::O_WRONLY == OFlag::O_WRONLY {
            oflags = (oflags & !OFlag::O_WRONLY) | OFlag::O_RDWR;
        }

        let res = tryfuse!(
            fcntl::open(Path::new(&path), oflags, stat::Mode::empty()),
            reply
        );

        // avoid double caching
        tryfuse!(posix_fadvise(res), reply);
        let fh = Fh::new(Fd::new(res, FdState::from(oflags)));
        reply.opened(Box::into_raw(fh) as u64, fuse::consts::FOPEN_KEEP_CACHE); // freed by close
    }

    fn read(
        &mut self,
        _req: &Request,
        _ino: u64,
        fh: u64,
        offset: i64,
        size: u32,
        reply: ReplyRead,
    ) {
        fsuid::set_root();

        if self.splice_read {
            reply.fd(get_filehandle(fh).fd.raw(), offset, size);
        } else {
            let mut v = vec![0; size as usize];
            let buf = v.as_mut_slice();
            tryfuse!(pread(get_filehandle(fh).fd.raw(), buf, offset), reply);

            reply.data(buf);
        }
    }

    fn write(
        &mut self,
        _req: &Request,
        _ino: u64,
        fh: u64,
        mut offset: i64,
        _fd: Option<RawFd>,
        data: &[u8],
        size: u32,
        _flags: u32,
        reply: ReplyWrite,
    ) {
        fsuid::set_root();
        let dst_fd = get_filehandle(fh).fd.raw();

        let written = if let Some(fd) = _fd {
            tryfuse!(
                fcntl::splice(
                    fd,
                    None,
                    dst_fd,
                    Some(&mut offset),
                    size as usize,
                    // SPLICE_F_MOVE is a no-op in the kernel at the moment according to manpage
                    SpliceFFlags::SPLICE_F_MOVE | SpliceFFlags::SPLICE_F_NONBLOCK,
                ),
                reply
            )
        } else {
            tryfuse!(pwrite(dst_fd, data, offset), reply)
        };

        reply.written(written as u32);
    }

    fn flush(&mut self, _req: &Request, _ino: u64, fh: u64, _lock_owner: u64, reply: ReplyEmpty) {
        fsuid::set_root();

        let handle = get_filehandle(fh);

        match unistd::dup(handle.fd.raw()) {
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
        fsuid::set_root();
        unsafe { drop(Box::from_raw(fh as *mut Fh)) };
        reply.ok();
    }

    fn fsync(&mut self, _req: &Request, _ino: u64, fh: u64, datasync: bool, reply: ReplyEmpty) {
        fsuid::set_root();

        let handle = get_filehandle(fh);

        let fd = handle.fd.raw();
        if datasync {
            tryfuse!(unistd::fsync(fd), reply);
        } else {
            tryfuse!(unistd::fdatasync(fd), reply);
        }

        reply.ok();
    }

    fn opendir(&mut self, _req: &Request, mut ino: u64, _flags: u32, reply: ReplyOpen) {
        fsuid::set_root();

        let inode = tryfuse!(self.mutable_inode(&mut ino), reply);
        let fd = inode.fd.read();
        let path = fd_path(&fd);
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
        _req: &Request,
        ino: u64,
        fh: u64,
        offset: i64,
        reply: fuse::ReplyDirectory,
    ) {
        self.generic_readdir(ino, fh, offset, ReplyDirectory::Directory(reply))
    }

    fn readdirplus(
        &mut self,
        _req: &Request,
        ino: u64,
        fh: u64,
        offset: i64,
        reply: fuse::ReplyDirectoryPlus,
    ) {
        self.generic_readdir(ino, fh, offset, ReplyDirectory::DirectoryPlus(reply))
    }

    fn releasedir(&mut self, _req: &Request, _ino: u64, fh: u64, _flags: u32, reply: ReplyEmpty) {
        fsuid::set_root();

        let dirp = unsafe { Box::from_raw(fh as *mut DirP) };
        assert!(dirp.magic == DIRP_MAGIC);
        // dirp out-of-scope -> closedir(dirp.dp)
        reply.ok();
    }

    fn fsyncdir(&mut self, _req: &Request, _ino: u64, fh: u64, datasync: bool, reply: ReplyEmpty) {
        fsuid::set_root();

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

    fn statfs(&mut self, _req: &Request, mut ino: u64, reply: ReplyStatfs) {
        fsuid::set_root();

        let inode = tryfuse!(self.mutable_inode(&mut ino), reply);

        let fd = inode.fd.read();
        let stat = tryfuse!(fstatvfs(fd.raw()), reply);
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

    fn getxattr(
        &mut self,
        _req: &Request,
        mut ino: u64,
        name: &OsStr,
        size: u32,
        reply: ReplyXattr,
    ) {
        fsuid::set_root();

        let inode = tryfuse!(self.inode(&mut ino), reply);
        let fd = inode.fd.read();

        if size == 0 {
            let size = tryfuse!(xattr::getxattr(&fd, inode.kind, name, &mut []), reply);
            reply.size(size as u32);
        } else {
            let mut buf = vec![0; size as usize];
            let size = tryfuse!(
                xattr::getxattr(&fd, inode.kind, name, buf.as_mut_slice()),
                reply
            );
            reply.data(&buf[..size]);
        }
    }

    fn listxattr(&mut self, _req: &Request, mut ino: u64, size: u32, reply: ReplyXattr) {
        fsuid::set_root();

        let inode = tryfuse!(self.inode(&mut ino), reply);
        let fd = inode.fd.read();

        if size == 0 {
            let res = xattr::listxattr(&fd, inode.kind, &mut []);
            let size = tryfuse!(res, reply);
            reply.size(size as u32);
        } else {
            let mut buf = vec![0; size as usize];
            let size = tryfuse!(xattr::listxattr(&fd, inode.kind, buf.as_mut_slice()), reply);
            reply.data(&buf[..size]);
        }
    }

    fn setxattr(
        &mut self,
        _req: &Request,
        mut ino: u64,
        name: &OsStr,
        value: &[u8],
        flags: u32,
        _position: u32,
        reply: ReplyEmpty,
    ) {
        fsuid::set_root();

        let inode = tryfuse!(self.inode(&mut ino), reply);
        let fd = inode.fd.read();

        if name == POSIX_ACL_DEFAULT_XATTR {
            let mut default_acl = inode.has_default_acl.write();
            tryfuse!(xattr::setxattr(&fd, inode.kind, name, value, flags), reply);
            *default_acl = Some(true);
        } else {
            tryfuse!(xattr::setxattr(&fd, inode.kind, name, value, flags), reply);
        }

        reply.ok();
    }

    fn removexattr(&mut self, _req: &Request, mut ino: u64, name: &OsStr, reply: ReplyEmpty) {
        fsuid::set_root();

        let inode = tryfuse!(self.inode(&mut ino), reply);
        let fd = inode.fd.read();

        if name == POSIX_ACL_DEFAULT_XATTR {
            let mut default_acl = inode.has_default_acl.write();
            tryfuse!(xattr::removexattr(&fd, inode.kind, name), reply);
            *default_acl = Some(false);
        } else {
            tryfuse!(xattr::removexattr(&fd, inode.kind, name), reply);
        }

        reply.ok();
    }

    fn access(&mut self, _req: &Request, ino: u64, mask: u32, reply: ReplyEmpty) {
        fsuid::set_root();

        let inode = tryfuse!(self.inode(&ino), reply);
        let mode = unistd::AccessMode::from_bits_truncate(mask as i32);
        tryfuse!(
            unistd::access(fd_path(&inode.fd.read()).as_str(), mode),
            reply
        );
        reply.ok();
    }

    fn create(
        &mut self,
        req: &Request,
        parent: u64,
        name: &OsStr,
        mode: u32,
        umask: u32,
        flags: u32,
        reply: ReplyCreate,
    ) {
        let fd = tryfuse!(
            self.create_file(req, parent, name, mode, umask, flags),
            reply
        );

        let new_file = unsafe { File::from_raw_fd(fd) };
        let (attr, generation) =
            tryfuse!(self.lookup_from_fd(LookupFile::Borrow(&new_file)), reply);
        let fh = Fh::new(Fd::new(new_file.into_raw_fd(), FdState::Readable));

        let fp = Box::into_raw(fh) as u64; // freed by close
        reply.created(&TTL, &attr, generation, fp, flags);
    }

    // we do not support remote locking at the moment and rely on the kernel
    //use fuse::ReplyLock;
    //fn getlk(
    //    &mut self,
    //    _req: &Request,
    //    _ino: u64,
    //    fh: u64,
    //    _lock_owner: u64,
    //    start: u64,
    //    end: u64,
    //    typ: u32,
    //    pid: u32,
    //    reply: ReplyLock,
    //) {
    //    fsuid::set_root();

    //    let handle = get_filehandle(fh);
    //    let mut flock = libc::flock {
    //        l_type: typ as i16,
    //        l_whence: 0,
    //        l_start: start as i64,
    //        l_len: (end - start) as i64,
    //        l_pid: pid as i32,
    //    };
    //    tryfuse!(
    //        fcntl::fcntl(handle.fd.raw(), fcntl::F_GETLK(&mut flock)),
    //        reply
    //    );
    //    reply.locked(
    //        flock.l_start as u64,
    //        (flock.l_start + flock.l_len) as u64,
    //        flock.l_type as u32,
    //        flock.l_pid as u32,
    //    )
    //}

    //fn setlk(
    //    &mut self,
    //    _req: &Request,
    //    _ino: u64,
    //    fh: u64,
    //    _lock_owner: u64,
    //    start: u64,
    //    end: u64,
    //    typ: u32,
    //    pid: u32,
    //    _sleep: bool,
    //    reply: ReplyEmpty,
    //) {
    //    fsuid::set_root();

    //    let handle = get_filehandle(fh);
    //    let flock = libc::flock {
    //        l_type: typ as i16,
    //        l_whence: 0,
    //        l_start: start as i64,
    //        l_len: (end - start) as i64,
    //        l_pid: pid as i32,
    //    };
    //    tryfuse!(fcntl::fcntl(handle.fd.raw(), fcntl::F_SETLK(&flock)), reply);
    //    reply.ok()
    //}

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
        fsuid::set_root();

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
        _req: &Request,
        _ino: u64,
        fh: u64,
        flags: u32,
        _cmd: u32,
        in_data: Option<&[u8]>,
        out_size: u32,
        reply: ReplyIoctl,
    ) {
        fsuid::set_root();

        let fd = if (flags & fuse::consts::FUSE_IOCTL_DIR) > 0 {
            let dirp = unsafe { &mut (*(fh as *mut DirP)) };
            assert!(dirp.magic == DIRP_MAGIC);
            tryfuse!(dirent::dirfd(&mut dirp.dp), reply)
        } else {
            get_filehandle(fh).fd.raw()
        };

        let cmd = u64::from(_cmd);

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

    fn lseek(
        &mut self,
        _req: &Request,
        _ino: u64,
        fh: u64,
        offset: i64,
        whence: u32,
        reply: ReplyLseek,
    ) {
        fsuid::set_root();

        let fd = get_filehandle(fh).fd.raw();
        let new_offset = tryfuse!(
            unistd::lseek64(fd, offset, unsafe { mem::transmute(whence as i32) }),
            reply
        );
        reply.offset(new_offset);
    }
}
