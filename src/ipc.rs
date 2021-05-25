use nix::errno::Errno;
use nix::sys::socket::*;
use nix::sys::uio::IoVec;
use simple_error::try_with;
use std::fs::File;
use std::os::unix::prelude::*;

use crate::result::Result;

pub struct Socket {
    fd: File,
}

impl Socket {
    pub fn send(&self, messages: &[&[u8]], files: &[&File]) -> Result<()> {
        let iov: Vec<IoVec<&[u8]>> = messages.iter().map(|m| IoVec::from_slice(m)).collect();
        let fds: Vec<RawFd> = files.iter().map(|f| f.as_raw_fd()).collect();
        let cmsg = if files.is_empty() {
            vec![]
        } else {
            vec![ControlMessage::ScmRights(&fds)]
        };

        try_with!(
            sendmsg(self.fd.as_raw_fd(), &iov, &cmsg, MsgFlags::empty(), None),
            "sendmsg failed"
        );
        Ok(())
    }

    pub fn receive(
        &self,
        message_length: usize,
        cmsgspace: &mut Vec<u8>,
    ) -> Result<(Vec<u8>, Vec<File>)> {
        let mut msg_buf = vec![0; (message_length) as usize];
        let received;
        let mut files: Vec<File> = Vec::with_capacity(1);
        {
            let iov = [IoVec::from_mut_slice(&mut msg_buf)];
            loop {
                match recvmsg(
                    self.fd.as_raw_fd(),
                    &iov,
                    Some(&mut *cmsgspace),
                    MsgFlags::empty(),
                ) {
                    Err(nix::Error::Sys(Errno::EAGAIN)) | Err(nix::Error::Sys(Errno::EINTR)) => {
                        continue
                    }
                    Err(e) => return try_with!(Err(e), "recvmsg failed"),
                    Ok(msg) => {
                        for cmsg in msg.cmsgs() {
                            if let ControlMessageOwned::ScmRights(fds) = cmsg {
                                for fd in fds {
                                    files.push(unsafe { File::from_raw_fd(fd) })
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
            fd: unsafe { File::from_raw_fd(parent_fd) },
        },
        Socket {
            fd: unsafe { File::from_raw_fd(child_fd) },
        },
    ))
}
