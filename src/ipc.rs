use anyhow::Context;
use rustix::cmsg_space;
use rustix::io::Errno;
use rustix::net::{
    AddressFamily, RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, SendAncillaryBuffer,
    SendAncillaryMessage, SendFlags, SocketFlags, SocketType, recvmsg, sendmsg, socketpair,
};
use std::io::{IoSlice, IoSliceMut};
use std::mem::MaybeUninit;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};

use crate::result::Result;

pub(crate) struct Socket {
    fd: OwnedFd,
}

/// Maximum number of file descriptors transferred per message
const MAX_FDS: usize = 2;

impl Socket {
    /// Send file descriptors using SCM_RIGHTS
    pub(crate) fn send<F: AsFd>(&self, messages: &[&[u8]], files: &[&F]) -> Result<()> {
        let iov: Vec<IoSlice> = messages.iter().map(|m| IoSlice::new(m)).collect();
        let fds: Vec<BorrowedFd> = files.iter().map(|f| f.as_fd()).collect();
        assert!(fds.len() <= MAX_FDS);

        let mut space = [MaybeUninit::uninit(); cmsg_space!(ScmRights(MAX_FDS))];
        let mut cmsg_buffer = SendAncillaryBuffer::new(&mut space);
        if !fds.is_empty() {
            cmsg_buffer.push(SendAncillaryMessage::ScmRights(&fds));
        }

        sendmsg(&self.fd, &iov, &mut cmsg_buffer, SendFlags::empty())
            .context("failed to send message via Unix socket")?;
        Ok(())
    }

    /// Receive file descriptors using SCM_RIGHTS
    ///
    /// Works with any type constructible from an OwnedFd (File, OwnedFd, etc.)
    pub(crate) fn receive<F: From<OwnedFd>>(
        &self,
        message_length: usize,
    ) -> Result<(Vec<u8>, Vec<F>)> {
        let mut msg_buf = vec![0; message_length];
        let mut space = [MaybeUninit::uninit(); cmsg_space!(ScmRights(MAX_FDS))];
        let mut fds: Vec<OwnedFd> = Vec::with_capacity(1);

        let received = {
            let mut iov = [IoSliceMut::new(&mut msg_buf)];
            let mut cmsg_buffer = RecvAncillaryBuffer::new(&mut space);
            loop {
                match recvmsg(&self.fd, &mut iov, &mut cmsg_buffer, RecvFlags::empty()) {
                    Err(Errno::AGAIN) | Err(Errno::INTR) => continue,
                    Err(e) => return Err(e).context("failed to receive message from Unix socket"),
                    Ok(msg) => {
                        for cmsg in cmsg_buffer.drain() {
                            if let RecvAncillaryMessage::ScmRights(received_fds) = cmsg {
                                fds.extend(received_fds);
                            }
                        }
                        break msg.bytes;
                    }
                }
            }
        };
        msg_buf.resize(received, 0);

        // Convert received FDs to the desired type
        let files = fds.into_iter().map(F::from).collect();

        Ok((msg_buf, files))
    }
}

pub(crate) fn socket_pair() -> Result<(Socket, Socket)> {
    let (parent_fd, child_fd) = socketpair(
        AddressFamily::UNIX,
        SocketType::SEQPACKET, // Use SEQPACKET instead of DGRAM for EOF detection
        SocketFlags::CLOEXEC,
        None,
    )
    .context("failed to create socketpair")?;
    Ok((Socket { fd: parent_fd }, Socket { fd: child_fd }))
}
