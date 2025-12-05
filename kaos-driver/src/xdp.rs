//! AF_XDP kernel bypass networking

use std::io;
use std::num::NonZeroU32;

/// XDP socket configuration
#[derive(Debug, Clone)]
pub struct XdpConfig {
    pub interface: String,
    pub queue_id: u32,
    pub frame_size: u32,
    pub frame_count: u32,
    pub batch_size: usize,
}

/// Default XDP frame size (4KB = page size, good for most NICs)
const DEFAULT_FRAME_SIZE: u32 = 4096;

/// Default frame count (4K frames = 16MB UMEM)
const DEFAULT_FRAME_COUNT: u32 = 4096;

/// Default batch size for TX/RX operations
const DEFAULT_BATCH_SIZE: usize = 64;

impl Default for XdpConfig {
    fn default() -> Self {
        Self {
            interface: "lo".to_string(),
            queue_id: 0,
            frame_size: DEFAULT_FRAME_SIZE,
            frame_count: DEFAULT_FRAME_COUNT,
            batch_size: DEFAULT_BATCH_SIZE,
        }
    }
}

#[cfg(all(target_os = "linux", feature = "xdp"))]
mod inner {
    use super::*;
    use xsk_rs::{
        config::{FrameSize, Interface, QueueSize, SocketConfig, UmemConfig},
        socket::Socket,
        umem::Umem,
        CompQueue, FillQueue, FrameDesc, RxQueue, TxQueue,
    };

    pub struct XdpSocket {
        _umem: Umem,
        fill_q: FillQueue,
        comp_q: CompQueue,
        tx_q: TxQueue,
        rx_q: RxQueue,
        frames: Vec<FrameDesc>,
        batch_size: usize,
        tx_idx: usize,
    }

    impl XdpSocket {
        pub fn new(config: XdpConfig) -> io::Result<Self> {
            let frame_count = NonZeroU32::new(config.frame_count)
                .ok_or_else(|| io::Error::other("frame_count must be > 0"))?;
            // UMEM config
            let frame_size = FrameSize::new(config.frame_size)
                .map_err(|e| io::Error::other(format!("FrameSize: {:?}", e)))?;

            let umem_config = UmemConfig::builder()
                .frame_size(frame_size)
                .build()
                .map_err(|e| io::Error::other(format!("UmemConfig: {:?}", e)))?;

            // Create UMEM - takes (config, frame_count, hugetlb)
            let (umem, frames) = Umem::new(umem_config, frame_count, false)
                .map_err(|e| io::Error::other(format!("Umem: {:?}", e)))?;

            // Parse interface
            let iface: Interface = config
                .interface
                .parse()
                .map_err(|e| io::Error::other(format!("Interface: {:?}", e)))?;

            // Queue size
            let q_size = QueueSize::new(config.frame_count)
                .map_err(|e| io::Error::other(format!("QueueSize: {:?}", e)))?;

            // Socket config
            let socket_config = SocketConfig::builder()
                .rx_queue_size(q_size)
                .tx_queue_size(q_size)
                .build();

            // Create socket
            let (tx_q, rx_q, fq_cq) =
                (unsafe { Socket::new(socket_config, &umem, &iface, config.queue_id) })
                    .map_err(|e| io::Error::other(format!("Socket: {:?}", e)))?;

            // Get fill/comp queues
            let (fill_q, comp_q) = fq_cq.ok_or_else(|| io::Error::other("no fill/comp queues"))?;

            // Pre-fill RX queue
            let mut fill_q = fill_q;
            let half = frames.len() / 2;
            unsafe {
                fill_q.produce(&frames[..half]);
            }

            Ok(Self {
                _umem: umem,
                fill_q,
                comp_q,
                tx_q,
                rx_q,
                frames,
                batch_size: config.batch_size,
                tx_idx: half,
            })
        }

        pub fn send(&mut self, _data: &[u8]) -> io::Result<usize> {
            if self.tx_idx >= self.frames.len() {
                let mut completed = vec![FrameDesc::default(); self.batch_size];
                let n = unsafe { self.comp_q.consume(&mut completed) };
                if n == 0 {
                    return Ok(0);
                }
                self.tx_idx = self.frames.len() / 2;
            }

            let frame = self.frames[self.tx_idx].clone();
            self.tx_idx += 1;

            unsafe { Ok(self.tx_q.produce(&[frame])) }
        }

        pub fn recv(&mut self) -> usize {
            let mut descs = vec![FrameDesc::default(); self.batch_size];
            unsafe {
                let n = self.rx_q.consume(&mut descs);
                if n > 0 {
                    self.fill_q.produce(&descs[..n]);
                }
                n
            }
        }

        pub fn poll(&mut self) -> usize {
            let mut descs = vec![FrameDesc::default(); self.batch_size];
            unsafe { self.comp_q.consume(&mut descs) }
        }
    }
}

#[cfg(all(target_os = "linux", feature = "xdp"))]
pub use inner::XdpSocket;

#[cfg(not(all(target_os = "linux", feature = "xdp")))]
pub struct XdpSocket;

#[cfg(not(all(target_os = "linux", feature = "xdp")))]
impl XdpSocket {
    pub fn new(_: XdpConfig) -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "AF_XDP requires Linux",
        ))
    }
    pub fn send(&mut self, _: &[u8]) -> io::Result<usize> {
        Ok(0)
    }
    pub fn recv(&mut self) -> usize {
        0
    }
    pub fn poll(&mut self) -> usize {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_config() {
        assert_eq!(XdpConfig::default().frame_size, 4096);
    }
}
