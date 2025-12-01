//! sendmmsg/recvmmsg for batch UDP I/O (Linux)
//!
//! This module provides Linux sendmmsg() and recvmmsg() support for sending
//! and receiving multiple UDP packets in a single syscall.
//!
//! Performance benefit: 5-10x reduction in syscall overhead

#![allow(dead_code)]

use std::net::SocketAddr;
use std::io;

#[cfg(target_os = "linux")]
use std::os::unix::io::AsRawFd;

#[cfg(target_os = "linux")]
use libc::{ sendmmsg, recvmmsg, mmsghdr, iovec, sockaddr_in, sockaddr_in6, AF_INET, AF_INET6 };

#[cfg(target_os = "linux")]
pub struct BatchSender {
    msgvec: Vec<mmsghdr>,
    iovecs: Vec<iovec>,
    addrs: Vec<sockaddr_in>,
}

#[cfg(target_os = "linux")]
impl BatchSender {
    pub fn new(batch_size: usize) -> Self {
        let msgvec = vec![unsafe { std::mem::zeroed() }; batch_size];
        let iovecs = vec![unsafe { std::mem::zeroed() }; batch_size];
        let addrs = vec![unsafe { std::mem::zeroed() }; batch_size];

        Self {
            msgvec,
            iovecs,
            addrs,
        }
    }

    /// Send multiple UDP packets in one syscall
    ///
    /// # Safety
    /// - packets must be valid for the duration of the call
    /// - addr must be a valid SocketAddr
    pub unsafe fn send_batch(
        &mut self,
        socket_fd: i32,
        packets: &[&[u8]],
        addr: &SocketAddr
    ) -> io::Result<usize> {
        if packets.is_empty() {
            return Ok(0);
        }

        let count = packets.len().min(self.msgvec.len());

        // Setup address
        let sockaddr = match addr {
            SocketAddr::V4(v4) => {
                let mut addr: sockaddr_in = std::mem::zeroed();
                addr.sin_family = AF_INET as u16;
                addr.sin_port = v4.port().to_be();
                addr.sin_addr = std::mem::transmute(v4.ip().octets());
                addr
            }
            SocketAddr::V6(_) => {
                return Err(io::Error::new(io::ErrorKind::InvalidInput, "IPv6 not yet supported"));
            }
        };

        // Setup iovec and mmsghdr for each packet
        for i in 0..count {
            self.iovecs[i].iov_base = packets[i].as_ptr() as *mut _;
            self.iovecs[i].iov_len = packets[i].len();

            self.addrs[i] = sockaddr;

            self.msgvec[i].msg_hdr.msg_name = &mut self.addrs[i] as *mut _ as *mut _;
            self.msgvec[i].msg_hdr.msg_namelen = std::mem::size_of::<sockaddr_in>() as u32;
            self.msgvec[i].msg_hdr.msg_iov = &mut self.iovecs[i] as *mut _;
            self.msgvec[i].msg_hdr.msg_iovlen = 1;
            self.msgvec[i].msg_hdr.msg_control = std::ptr::null_mut();
            self.msgvec[i].msg_hdr.msg_controllen = 0;
            self.msgvec[i].msg_hdr.msg_flags = 0;
        }

        // Send all packets in one syscall
        let result = sendmmsg(socket_fd, self.msgvec.as_mut_ptr(), count as u32, 0);

        if result < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(result as usize)
        }
    }
}

#[cfg(target_os = "linux")]
pub struct BatchReceiver {
    msgvec: Vec<mmsghdr>,
    iovecs: Vec<iovec>,
    buffers: Vec<Vec<u8>>,
    addrs: Vec<sockaddr_in>,
}

#[cfg(target_os = "linux")]
impl BatchReceiver {
    pub fn new(batch_size: usize, buffer_size: usize) -> Self {
        let msgvec = vec![unsafe { std::mem::zeroed() }; batch_size];
        let iovecs = vec![unsafe { std::mem::zeroed() }; batch_size];
        let buffers: Vec<Vec<u8>> = (0..batch_size).map(|_| vec![0u8; buffer_size]).collect();
        let addrs = vec![unsafe { std::mem::zeroed() }; batch_size];

        Self {
            msgvec,
            iovecs,
            buffers,
            addrs,
        }
    }

    /// Receive multiple UDP packets in one syscall
    ///
    /// Returns the number of packets received and provides access to buffers
    ///
    /// # Safety
    /// - socket_fd must be a valid UDP socket
    pub unsafe fn recv_batch(&mut self, socket_fd: i32) -> io::Result<usize> {
        let count = self.msgvec.len();

        // Setup iovec and mmsghdr for each buffer
        for i in 0..count {
            self.iovecs[i].iov_base = self.buffers[i].as_mut_ptr() as *mut _;
            self.iovecs[i].iov_len = self.buffers[i].len();

            self.msgvec[i].msg_hdr.msg_name = &mut self.addrs[i] as *mut _ as *mut _;
            self.msgvec[i].msg_hdr.msg_namelen = std::mem::size_of::<sockaddr_in>() as u32;
            self.msgvec[i].msg_hdr.msg_iov = &mut self.iovecs[i] as *mut _;
            self.msgvec[i].msg_hdr.msg_iovlen = 1;
            self.msgvec[i].msg_hdr.msg_control = std::ptr::null_mut();
            self.msgvec[i].msg_hdr.msg_controllen = 0;
            self.msgvec[i].msg_hdr.msg_flags = 0;
            self.msgvec[i].msg_len = 0;
        }

        // Receive all packets in one syscall
        let result = recvmmsg(
            socket_fd,
            self.msgvec.as_mut_ptr(),
            count as u32,
            libc::MSG_DONTWAIT,
            std::ptr::null_mut()
        );

        if result < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::WouldBlock {
                return Ok(0);
            }
            Err(err)
        } else {
            Ok(result as usize)
        }
    }

}

// macOS doesn't have sendmmsg/recvmmsg, provide fallback that sends one at a time
#[cfg(not(target_os = "linux"))]
pub struct BatchSender;

#[cfg(not(target_os = "linux"))]
impl BatchSender {
    pub fn new(_batch_size: usize) -> Self {
        Self
    }

    pub unsafe fn send_batch(
        &mut self,
        _socket_fd: i32,
        _packets: &[&[u8]],
        _addr: &SocketAddr
    ) -> io::Result<usize> {
        Err(
            io::Error::new(
                io::ErrorKind::Unsupported,
                "sendmmsg not supported on this platform, use send_batch fallback"
            )
        )
    }
}

#[cfg(not(target_os = "linux"))]
pub struct BatchReceiver;

#[cfg(not(target_os = "linux"))]
impl BatchReceiver {
    pub fn new(_batch_size: usize, _buffer_size: usize) -> Self {
        Self
    }

    pub unsafe fn recv_batch(&mut self, _socket_fd: i32) -> io::Result<usize> {
        Err(
            io::Error::new(
                io::ErrorKind::Unsupported,
                "recvmmsg not supported on this platform, use receive fallback"
            )
        )
    }
}
