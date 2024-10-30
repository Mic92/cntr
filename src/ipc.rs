use nix::errno::Errno;
use nix::sys::socket::*;
use simple_error::try_with;
use std::fs::File;
use std::io::{IoSlice, IoSliceMut};
use std::os::unix::prelude::*;

use crate::result::Result;

pub struct Socket {
    fd: File,
}

const NONE: Option<&UnixAddr> = None;

impl Socket {
    pub fn send(&self, messages: &[&[u8]], files: &[&File]) -> Result<()> {
        let iov: Vec<IoSlice> = messages.iter().map(|m| IoSlice::new(m)).collect();
        let fds: Vec<RawFd> = files.iter().map(|f| f.as_raw_fd()).collect();
        let cmsg = if files.is_empty() {
            vec![]
        } else {
            vec![ControlMessage::ScmRights(&fds)]
        };

        try_with!(
            sendmsg(self.fd.as_raw_fd(), &iov, &cmsg, MsgFlags::empty(), NONE),
            "sendmsg failed"
        );
        Ok(())
    }

    pub fn receive(
        &self,
        message_length: usize,
        cmsgspace: &mut Vec<u8>,
    ) -> Result<(Vec<u8>, Vec<File>)> {
        let mut msg_buf = vec![0; message_length];
        let received;
        let mut files: Vec<File> = Vec::with_capacity(1);
        {
            let mut iov = vec![IoSliceMut::new(&mut msg_buf)];
            loop {
                let res = recvmsg::<UnixAddr>(
                    self.fd.as_raw_fd(),
                    &mut iov[..],
                    Some(&mut *cmsgspace),
                    MsgFlags::empty(),
                );
                match res {
                    Err(Errno::EAGAIN) | Err(Errno::EINTR) => continue,
                    Err(e) => return try_with!(Err(e), "recvmsg failed"),
                    Ok(msg) => {
                        for cmsg in msg.cmsgs() {
                            for cmsg in cmsg {
                                if let ControlMessageOwned::ScmRights(fds) = cmsg {
                                    for fd in fds {
                                        files.push(unsafe { File::from_raw_fd(fd) });
                                    }
                                }
                            }
                        }
                        received = msg.bytes;
                        break;
                    }
                };
            }
        }
        msg_buf.resize(received, 0);
        Ok((msg_buf, files))
    }
}

pub fn socket_pair() -> Result<(Socket, Socket)> {
    let res = socketpair(
        AddressFamily::Unix,
        SockType::Datagram,
        None,
        SockFlag::SOCK_CLOEXEC,
    );

    let (parent_fd, child_fd) = try_with!(res, "failed to create socketpair");
    Ok((
        Socket {
            fd: File::from(parent_fd),
        },
        Socket {
            fd: File::from(child_fd),
        },
    ))
}
