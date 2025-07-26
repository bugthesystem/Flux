use std::net::UdpSocket;
use rand::{ thread_rng, Rng };

// Get a free address to bind to
pub fn free_addr() -> std::net::SocketAddr {
    let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
    let addr = sock.local_addr().unwrap();
    drop(sock);
    addr
}

// Decorrelated jitter implementation based on AWS Architecture Blog
// https://aws.amazon.com/blogs/architecture/exponential-backoff-and-jitter/
pub fn decorrelated_jitter(base: u64, cap: u64, _last_jitter: u64) -> u64 {
    let mut rng = thread_rng();
    let temp = std::cmp::min(cap, base * 3);
    let next_jitter = rng.gen_range(0..=temp);

    std::cmp::min(cap, base + next_jitter)
}
