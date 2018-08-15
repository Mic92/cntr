use mount_utils;
use nix;
use libc;
use nix::errno::Errno;
use nix::fcntl::{fcntl, OFlag, FcntlArg, splice, SpliceFFlags};
use nix::sys::epoll::{epoll_create1, epoll_ctl, epoll_wait, EpollCreateFlags, EpollOp, EpollEvent,
                      EpollFlags};
use nix::sys::socket::{accept4, SockFlag, socket, bind, listen, AddressFamily, SockType, connect,
                       SockAddr, getsockopt, sockopt};
use nix::unistd::pipe2;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::File;
use std::io;
use std::io::prelude::*;
use std::os::unix::io::AsRawFd;
use std::os::unix::prelude::*;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use tempdir::TempDir;
use types::{Result, Error};

struct Pipe {
    reader: File,
    writer: File,
    size: usize,
}

struct ShovelPair {
    source_fd: Option<Rc<File>>,
    target_fd: Option<Rc<File>>,
    pipe: Pipe,
    buffer_full: usize,
}

struct Connection {
    server_to_client: ShovelPair,
    client_to_server: ShovelPair,
}

enum SpliceResult {
    Written(usize),
    Closed,
    Error(nix::Error),
}

fn do_splice(source: &File, target: &File, amount: usize) -> SpliceResult {
    let res = splice(
        source.as_raw_fd(),
        None,
        target.as_raw_fd(),
        None,
        amount,
        SpliceFFlags::SPLICE_F_MOVE | SpliceFFlags::SPLICE_F_NONBLOCK,
    );

    match res {
        Ok(spliced) => {
            if spliced == 0 {
                SpliceResult::Closed
            } else {
                SpliceResult::Written(spliced)
            }
        },
        Err(nix::Error::Sys(Errno::EPIPE)) |
        Err(nix::Error::Sys(Errno::ECONNRESET)) => SpliceResult::Closed,
        Err(nix::Error::Sys(Errno::EAGAIN)) |
        Err(nix::Error::Sys(Errno::EINTR)) => SpliceResult::Written(0),
        Err(e) => SpliceResult::Error(e),
    }
}

impl ShovelPair {
    fn shovel(&mut self, context: &mut Context) -> Result<()> {
        loop {
            let mut shoveled = false;
            if self.target_fd.is_some() {
                let res = if let Some(ref fd) = self.source_fd {
                    Some(do_splice(
                        &fd,
                        &self.pipe.writer,
                        self.pipe.size - self.buffer_full,
                    ))
                } else {
                    None
                };

                if let Some(result) = res {
                    match result {
                        SpliceResult::Written(size) => {
                            self.buffer_full += size;
                            if size > 0 {
                                shoveled = true;
                            }
                        }
                        SpliceResult::Closed => {
                            if let Some(ref fd) = self.source_fd {
                                tryfmt!(
                                    context.remove_file(fd.as_raw_fd()),
                                    "failed to remove source file"
                                );
                            }
                            self.source_fd = None;
                        }
                        SpliceResult::Error(e) => return tryfmt!(Err(e), "failed to splice"),
                    };
                }
            }
            if self.buffer_full > 0 {
                let res = if let Some(ref fd) = self.target_fd {
                    Some(do_splice(&self.pipe.reader, &fd, self.buffer_full))
                } else {
                    None
                };

                if let Some(result) = res {
                    match result {
                        SpliceResult::Written(size) => {
                            self.buffer_full -= size;
                            if size > 0 {
                                shoveled = true;
                            }
                        }
                        SpliceResult::Closed => {
                            if let Some(ref fd) = self.target_fd {
                                tryfmt!(
                                    context.remove_file(fd.as_raw_fd()),
                                    "failed to remove target file"
                                );
                            }
                            self.target_fd = None;
                        }
                        SpliceResult::Error(e) => return tryfmt!(Err(e), "failed to splice"),
                    };
                }
            }
            if !shoveled {
                return Ok(());
            }
        }
    }
}

// take from systemd's socket-proxyd
const BUFFER_SIZE: usize = 256 * 1024;

impl Pipe {
    fn new() -> Result<Pipe> {
        let (read_fd, write_fd) = tryfmt!(
            pipe2(OFlag::O_CLOEXEC | OFlag::O_NONBLOCK),
            "failed to create pipe"
        );

        let (reader, writer) = unsafe { (File::from_raw_fd(read_fd), File::from_raw_fd(write_fd)) };

        let _ = fcntl(
            reader.as_raw_fd(),
            FcntlArg::F_SETPIPE_SZ(BUFFER_SIZE as i32),
        );

        let size = tryfmt!(
            fcntl(reader.as_raw_fd(), FcntlArg::F_GETPIPE_SZ),
            "failed to get pipe size"
        );

        Ok(Pipe {
            reader: reader,
            writer: writer,
            size: size as usize,
        })
    }
}

impl Connection {
    pub fn new(server_fd: Rc<File>, client_fd: Rc<File>) -> Result<Connection> {
        let client_to_server = ShovelPair {
            source_fd: Some(Rc::clone(&server_fd)),
            target_fd: Some(Rc::clone(&client_fd)),
            pipe: tryfmt!(Pipe::new(), "failed to create parent pipe"),
            buffer_full: 0,
        };
        let server_to_client = ShovelPair {
            source_fd: Some(client_fd),
            target_fd: Some(server_fd),
            pipe: tryfmt!(Pipe::new(), "failed to create parent pipe"),
            buffer_full: 0,
        };
        Ok(Connection {
            client_to_server,
            server_to_client,
        })
    }
}

trait Callback {
    fn process(&mut self, context: &mut Context, flags: EpollFlags) -> Result<()>;
}

struct Context {
    epoll_file: File,
    callbacks: HashMap<RawFd, Rc<RefCell<Callback>>>,
}

impl Context {
    pub fn new() -> Result<Context> {
        let fd = tryfmt!(
            epoll_create1(EpollCreateFlags::EPOLL_CLOEXEC),
            "failed to create epoll socket"
        );
        Ok(Context {
            epoll_file: unsafe { File::from_raw_fd(fd) },
            callbacks: HashMap::new(),
        })
    }

    pub fn add_file(&mut self, fd: RawFd, callback: Rc<RefCell<Callback>>) -> Result<()> {
        let mut event = EpollEvent::new(EpollFlags::EPOLLIN | EpollFlags::EPOLLERR, fd as u64);

        let res = epoll_ctl(
            self.epoll_file.as_raw_fd(),
            EpollOp::EpollCtlAdd,
            fd,
            &mut event,
        );
        tryfmt!(res, "failed to add file descriptor to epoll socket");

        let old_file = self.callbacks.insert(fd, callback);

        assert!(old_file.is_none());
        Ok(())
    }

    pub fn remove_file(&mut self, id: i32) -> Result<()> {
        self.callbacks.remove(&id);
        tryfmt!(
            epoll_ctl(self.epoll_file.as_raw_fd(), EpollOp::EpollCtlDel, id, None),
            "failed to remove file"
        );
        Ok(())
    }

    pub fn select<'b>(&self, events: &'b mut [EpollEvent]) -> Result<&'b [EpollEvent]> {
        let res = epoll_wait(self.epoll_file.as_raw_fd(), events, -1);
        match res {
            Ok(count) => {
                Ok(&events[..count])
            },
            Err(nix::Error::Sys(Errno::EINTR)) => {
                Ok(&events[..0])
            },
            Err(e) => {
                tryfmt!(Err(e), "failed to wait for epoll events")
            },
        }
    }
}

struct ConnectCb {
    server_file: Rc<File>,
    client_file: Rc<File>,
    address: PathBuf,
}

impl Callback for ConnectCb {
    fn process(&mut self, context: &mut Context, _flags: EpollFlags) -> Result<()> {
        let error = tryfmt!(
            getsockopt(self.client_file.as_raw_fd(), sockopt::SocketError),
            "failed to get socket option SO_ERROR"
        );
        if error != 0 {
            warn!(
                "failed to connect to socket '{}': {}",
                self.address.display(),
                Errno::from_i32(error)
            );
        }

        tryfmt!(
            context.remove_file(self.client_file.as_raw_fd()),
            "failed to remove file from queue"
        );

        tryfmt!(
            on_connection_complete(
                context,
                Rc::clone(&self.server_file),
                Rc::clone(&self.client_file),
            ),
            "failed to set up connection"
        );
        Ok(())
    }
}

struct AcceptCb {
    listener: Listener,
}

fn on_connection_complete(
    context: &mut Context,
    server_file: Rc<File>,
    client_file: Rc<File>,
) -> Result<()> {
    let server_fd = server_file.as_raw_fd();
    let client_fd = client_file.as_raw_fd();
    let connection = Rc::new(RefCell::new(tryfmt!(
        Connection::new(server_file, client_file),
        "failed to setup file pair"
    )));
    let cb2 = Rc::clone(&connection);
    tryfmt!(
        context.add_file(server_fd, cb2),
        "failed to watch server socket"
    );
    tryfmt!(
        context.add_file(client_fd, connection),
        "failed to watch client socket"
    );
    Ok(())
}

impl Callback for AcceptCb {
    fn process(&mut self, context: &mut Context, flags: EpollFlags) -> Result<()> {
        assert!(flags == EpollFlags::EPOLLIN);
        let server_file = match accept4(
            self.listener.socket.as_raw_fd(),
            SockFlag::SOCK_NONBLOCK | SockFlag::SOCK_CLOEXEC,
        ) {
            Ok(fd) => unsafe { File::from_raw_fd(fd) },
            Err(nix::Error::Sys(Errno::EAGAIN)) => {
                return Ok(());
            }
            Err(err) => tryfmt!(Err(err), "failed to accept connections on socket"),
        };

        let client_file = match socket(
            AddressFamily::Unix,
            SockType::Stream,
            SockFlag::SOCK_NONBLOCK | SockFlag::SOCK_CLOEXEC,
            None,
        ) {
            Err(e) => {
                warn!(
                    "Failed to get remote socket for '{}': {}",
                    self.listener.address.display(),
                    e
                );
                return Ok(());
            }
            Ok(fd) => unsafe { File::from_raw_fd(fd) },
        };
        let addr = tryfmt!(
            SockAddr::new_unix(&self.listener.address),
            "failed to connect to {}",
            self.listener.address.display()
        );
        match connect(client_file.as_raw_fd(), &addr) {
            Ok(()) => {
                on_connection_complete(context, Rc::new(server_file), Rc::new(client_file))
            }
            Err(nix::Error::Sys(Errno::EINPROGRESS)) => {
                let client_fd = client_file.as_raw_fd();
                let client_file = Rc::new(client_file);
                let cb = Rc::new(RefCell::new(ConnectCb {
                    client_file,
                    server_file: Rc::new(server_file),
                    address: self.listener.address.to_owned(),
                }));
                tryfmt!(
                    context.add_file(client_fd, cb),
                    "failed to add file to queue"
                );
                Ok(())
            }
            Err(err) => {
                warn!(
                    "Failed to connect to '{}': {}",
                    self.listener.address.as_path().display(),
                    err
                );
                Ok(())
            }
        }
    }
}

impl Callback for Connection {
    fn process(&mut self, context: &mut Context, _flags: EpollFlags) -> Result<()> {

        tryfmt!(
            self.server_to_client.shovel(context),
            "failed to transfer from server to client"
        );

        tryfmt!(
            self.client_to_server.shovel(context),
            "failed to transfer from client to server"
        );

        Ok(())
    }
}

pub struct Awakener {
    pipe: Pipe,
}

impl Awakener {
    pub fn new() -> Result<Awakener> {
        let (read_fd, write_fd) = tryfmt!(
            pipe2(OFlag::O_CLOEXEC | OFlag::O_NONBLOCK),
            "failed to create pipe"
        );
        let pipe = unsafe {
            Pipe {
                reader: File::from_raw_fd(read_fd),
                writer: File::from_raw_fd(write_fd),
                size: 0,
            }
        };
        Ok(Awakener { pipe })
    }

    pub fn wakeup(&self) -> io::Result<()> {
        match (&self.pipe.writer).write(&[1]) {
            Ok(_) => Ok(()),
            Err(e) => {
                if e.kind() == io::ErrorKind::WouldBlock {
                    Ok(())
                } else {
                    Err(e)
                }
            }
        }
    }
}

pub struct NoopCallback {}

impl Callback for NoopCallback {
    fn process(&mut self, _context: &mut Context, _flags: EpollFlags) -> Result<()> {
        Ok(())
    }
}

pub fn bind_paths(paths: &[PathBuf]) -> Result<(TempDir, Vec<Listener>)> {
    let socket_path = tryfmt!(
        TempDir::new("cntrfs-sockets"),
        "failed to create socket directory"
    );
    let mut listeners = vec![];

    for (i, target_path) in paths.iter().enumerate() {
        let fd = tryfmt!(
            socket(
                AddressFamily::Unix,
                SockType::Stream,
                SockFlag::SOCK_NONBLOCK | SockFlag::SOCK_CLOEXEC,
                None,
            ),
            "failed to create socket"
        );

        let listener_file = unsafe { File::from_raw_fd(fd) };

        let source_path = socket_path.path().join(i.to_string());

        let socket = tryfmt!(
            SockAddr::new_unix(&source_path),
            "invalid socket path '{}'",
            source_path.display()
        );

        tryfmt!(
            bind(listener_file.as_raw_fd(), &socket),
            "failed to bind socket '{}'",
            source_path.display()
        );

        let res = mount_utils::bind_mount(&source_path, target_path);

        match res {
            Err(nix::Error::Sys(Errno::ENOENT)) => {},
            Err(e) => {
                eprintln!("could not bind mount {}: {}", target_path.display(), e);
            },
            Ok(_) => {
                listeners.push(Listener {
                    address: target_path.to_owned(),
                    socket: listener_file,
                });
            }
        };
    }


    Ok((socket_path, listeners))
}

fn setup_context(listeners: Vec<Listener>, awakener: &Awakener) -> Result<Context> {
    let mut context = tryfmt!(Context::new(), "failed to create epoll context");

    for listener in listeners {
        let fd = listener.socket.as_raw_fd();

        tryfmt!(listen(fd, libc::SOMAXCONN as usize), "failed to listen on socket '{}'", listener.address.display());

        let accept_cb = Rc::new(RefCell::new(AcceptCb { listener }));
        tryfmt!(
            context.add_file(fd, accept_cb),
            "could not add unix socket to event loop"
        );
    }

    tryfmt!(
        context.add_file(
            awakener.pipe.reader.as_raw_fd(),
            Rc::new(RefCell::new(NoopCallback {})),
        ),
        "could not add awakener pipe to event loop"
    );

    Ok(context)
}

fn forward(mut context: Context, awakener: &Awakener) -> Result<()> {
    let mut events = vec![EpollEvent::empty(); 1024];

    loop {
        let selected_events = tryfmt!(
            context.select(&mut events),
            "failed to wait for listening sockets"
        );
        for event in selected_events {
            if event.data() == awakener.pipe.reader.as_raw_fd() as u64 {
                return Ok(());
            }
            let callback = Rc::clone(&context.callbacks[&(event.data() as RawFd)]);
            tryfmt!(
                callback.borrow_mut().process(&mut context, event.events()),
                "failed to process epoll event"
            );
        }
    }
}

pub struct SocketProxy {
    awakener: Arc<Awakener>,
    join_handle: Option<JoinHandle<Option<String>>>,
}

impl Drop for SocketProxy {
    fn drop(&mut self) {
        if let Err(e) = self.awakener.wakeup() {
            eprintln!("failed to awake proxy: {}", e);
        };
        let res = self.join_handle.take().unwrap().join();
        match res {
            Ok(Some(msg)) => {
                eprintln!("proxy thread failed: {}", msg);
            }
            Ok(None) => {}
            Err(e) => eprintln!("proxy thread failed: {:?}", e),
        }
    }
}

pub struct Listener {
    pub address: PathBuf,
    pub socket: File,
}


fn run(sockets: Vec<Listener>, awakener: Arc<Awakener>) -> Result<()> {
    let context = tryfmt!(
        setup_context(sockets, &awakener),
        "setup proxy forwarding failed"
    );
    tryfmt!(forward(context, &awakener), "forwarding failed");
    Ok(())
}

pub fn start(sockets: Vec<Listener>) -> Result<SocketProxy> {
    let awakener = Arc::new(tryfmt!(Awakener::new(), "failed to create awakener"));
    let awakener2 = Arc::clone(&awakener);
    let join_handle = thread::spawn(move || match run(sockets, awakener2) {
        Ok(()) => None,
        Err(e) => {
            eprintln!("listener failed: {}", e.to_string());
            None
        }
    });
    Ok(SocketProxy {
        join_handle: Some(join_handle),
        awakener,
    })
}
