use anyhow::Context;
use nix::errno::Errno;
use nix::sys::socket::*;
use std::fs::File;
use std::io::{IoSlice, IoSliceMut};
use std::os::unix::prelude::*;

use crate::result::Result;

pub struct Socket {
    fd: File,
}

const NONE: Option<&UnixAddr> = None;

impl Socket {
    /// Send file descriptors using SCM_RIGHTS
    ///
    /// Works with any type implementing AsRawFd (File, OwnedFd, etc.)
    pub fn send<F: AsRawFd>(&self, messages: &[&[u8]], files: &[&F]) -> Result<()> {
        let iov: Vec<IoSlice> = messages.iter().map(|m| IoSlice::new(m)).collect();
        let fds: Vec<RawFd> = files.iter().map(|f| f.as_raw_fd()).collect();
        let cmsg = if fds.is_empty() {
            vec![]
        } else {
            vec![ControlMessage::ScmRights(&fds)]
        };

        sendmsg(self.fd.as_raw_fd(), &iov, &cmsg, MsgFlags::empty(), NONE)
            .context("failed to send message via Unix socket")?;
        Ok(())
    }

    /// Receive file descriptors using SCM_RIGHTS
    ///
    /// Works with any type implementing FromRawFd (File, OwnedFd, etc.)
    pub fn receive<F: FromRawFd>(
        &self,
        message_length: usize,
        cmsgspace: &mut Vec<u8>,
    ) -> Result<(Vec<u8>, Vec<F>)> {
        let mut msg_buf = vec![0; message_length];
        let received;
        let mut fds: Vec<RawFd> = Vec::with_capacity(1);
        {
            let mut iov = [IoSliceMut::new(&mut msg_buf)];
            loop {
                let res = recvmsg::<UnixAddr>(
                    self.fd.as_raw_fd(),
                    &mut iov[..],
                    Some(&mut *cmsgspace),
                    MsgFlags::empty(),
                );
                match res {
                    Err(Errno::EAGAIN) | Err(Errno::EINTR) => continue,
                    Err(e) => return Err(e).context("failed to receive message from Unix socket"),
                    Ok(msg) => {
                        for cmsg in msg
                            .cmsgs()
                            .context("failed to get control messages from socket")?
                        {
                            if let ControlMessageOwned::ScmRights(received_fds) = cmsg {
                                for fd in received_fds {
                                    fds.push(fd);
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

        // Convert raw FDs to the desired type
        let files = fds
            .into_iter()
            .map(|fd| unsafe { F::from_raw_fd(fd) })
            .collect();

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

    let (parent_fd, child_fd) = res.context("failed to create socketpair")?;
    Ok((
        Socket {
            fd: File::from(parent_fd),
        },
        Socket {
            fd: File::from(child_fd),
        },
    ))
}
