//! sendmmsg/recvmmsg batch UDP I/O (Linux)
//!
//! 5-10x syscall reduction for bulk UDP.

use std::io;
use std::net::SocketAddr;

#[cfg(target_os = "linux")]
use libc::{iovec, mmsghdr, recvmmsg, sendmmsg, sockaddr_in, AF_INET};
#[cfg(target_os = "linux")]
use std::os::unix::io::AsRawFd;

#[cfg(target_os = "linux")]
pub struct BatchSender {
    msgvec: Vec<mmsghdr>,
    iovecs: Vec<iovec>,
    addrs: Vec<sockaddr_in>,
}

#[cfg(target_os = "linux")]
impl BatchSender {
    /// Create a new batch sender with the given batch size.
    ///
    /// # Panics
    /// Panics if `batch_size` is 0.
    pub fn new(batch_size: usize) -> Self {
        assert!(batch_size > 0, "batch_size must be > 0");
        // Safety: libc mmsghdr, iovec, sockaddr_in are valid when zeroed
        Self {
            msgvec: vec![unsafe { std::mem::zeroed() }; batch_size],
            iovecs: vec![unsafe { std::mem::zeroed() }; batch_size],
            addrs: vec![unsafe { std::mem::zeroed() }; batch_size],
        }
    }

    pub unsafe fn send_batch(
        &mut self,
        fd: i32,
        packets: &[&[u8]],
        addr: &SocketAddr,
    ) -> io::Result<usize> {
        if packets.is_empty() {
            return Ok(0);
        }
        let count = packets.len().min(self.msgvec.len());

        let sockaddr = match addr {
            SocketAddr::V4(v4) => {
                let mut a: sockaddr_in = std::mem::zeroed();
                a.sin_family = AF_INET as u16;
                a.sin_port = v4.port().to_be();
                a.sin_addr.s_addr = u32::from_ne_bytes(v4.ip().octets());
                a
            }
            SocketAddr::V6(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "IPv6 unsupported",
                ));
            }
        };

        for i in 0..count {
            self.iovecs[i].iov_base = packets[i].as_ptr() as *mut _;
            self.iovecs[i].iov_len = packets[i].len();
            self.addrs[i] = sockaddr;
            self.msgvec[i].msg_hdr.msg_name = &mut self.addrs[i] as *mut _ as *mut _;
            self.msgvec[i].msg_hdr.msg_namelen = std::mem::size_of::<sockaddr_in>() as u32;
            self.msgvec[i].msg_hdr.msg_iov = &mut self.iovecs[i] as *mut _;
            self.msgvec[i].msg_hdr.msg_iovlen = 1;
        }

        let r = sendmmsg(fd, self.msgvec.as_mut_ptr(), count as u32, 0);
        if r < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(r as usize)
        }
    }
}

// Safety: BatchSender owns all its data and doesn't share references across threads
#[cfg(target_os = "linux")]
unsafe impl Send for BatchSender {}

#[cfg(target_os = "linux")]
pub struct BatchReceiver {
    msgvec: Vec<mmsghdr>,
    iovecs: Vec<iovec>,
    buffers: Vec<Vec<u8>>,
    addrs: Vec<sockaddr_in>,
}

#[cfg(target_os = "linux")]
impl BatchReceiver {
    /// Create a new batch receiver.
    ///
    /// # Panics
    /// Panics if `batch_size` or `buffer_size` is 0.
    pub fn new(batch_size: usize, buffer_size: usize) -> Self {
        assert!(batch_size > 0, "batch_size must be > 0");
        assert!(buffer_size > 0, "buffer_size must be > 0");
        // Safety: libc mmsghdr, iovec, sockaddr_in are valid when zeroed
        Self {
            msgvec: vec![unsafe { std::mem::zeroed() }; batch_size],
            iovecs: vec![unsafe { std::mem::zeroed() }; batch_size],
            buffers: (0..batch_size).map(|_| vec![0u8; buffer_size]).collect(),
            addrs: vec![unsafe { std::mem::zeroed() }; batch_size],
        }
    }

    pub unsafe fn recv_batch(&mut self, fd: i32) -> io::Result<usize> {
        let count = self.msgvec.len();
        for i in 0..count {
            self.iovecs[i].iov_base = self.buffers[i].as_mut_ptr() as *mut _;
            self.iovecs[i].iov_len = self.buffers[i].len();
            self.msgvec[i].msg_hdr.msg_name = &mut self.addrs[i] as *mut _ as *mut _;
            self.msgvec[i].msg_hdr.msg_namelen = std::mem::size_of::<sockaddr_in>() as u32;
            self.msgvec[i].msg_hdr.msg_iov = &mut self.iovecs[i] as *mut _;
            self.msgvec[i].msg_hdr.msg_iovlen = 1;
            self.msgvec[i].msg_len = 0;
        }

        let r = recvmmsg(
            fd,
            self.msgvec.as_mut_ptr(),
            count as u32,
            libc::MSG_DONTWAIT,
            std::ptr::null_mut(),
        );
        if r < 0 {
            let e = io::Error::last_os_error();
            if e.kind() == io::ErrorKind::WouldBlock {
                Ok(0)
            } else {
                Err(e)
            }
        } else {
            Ok(r as usize)
        }
    }

    pub fn packet(&self, idx: usize) -> &[u8] {
        let len = self.msgvec[idx].msg_len as usize;
        &self.buffers[idx][..len]
    }
}

// Safety: BatchReceiver owns all its data and doesn't share references across threads
#[cfg(target_os = "linux")]
unsafe impl Send for BatchReceiver {}

// Non-Linux: stubs (API compatibility)
#[cfg(not(target_os = "linux"))]
#[allow(dead_code)]
pub struct BatchSender;

#[cfg(not(target_os = "linux"))]
#[allow(dead_code)]
impl BatchSender {
    pub fn new(_: usize) -> Self {
        Self
    }
    pub unsafe fn send_batch(&mut self, _: i32, _: &[&[u8]], _: &SocketAddr) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "sendmmsg: Linux only",
        ))
    }
}

#[cfg(not(target_os = "linux"))]
#[allow(dead_code)]
pub struct BatchReceiver;

#[cfg(not(target_os = "linux"))]
#[allow(dead_code)]
impl BatchReceiver {
    pub fn new(_: usize, _: usize) -> Self {
        Self
    }
    pub unsafe fn recv_batch(&mut self, _: i32) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "recvmmsg: Linux only",
        ))
    }
    pub fn packet(&self, _: usize) -> &[u8] {
        &[]
    }
}
