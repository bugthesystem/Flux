#!/bin/bash
# AF_XDP Test Script - Run on Linux with root privileges
# Usage: sudo ./test-xdp.sh

set -e

echo "=== AF_XDP Test Setup ==="

# Check prerequisites
if [[ "$(uname)" != "Linux" ]]; then
    echo "ERROR: AF_XDP requires Linux"
    exit 1
fi

if [[ $EUID -ne 0 ]]; then
    echo "ERROR: Must run as root (sudo)"
    exit 1
fi

KERNEL_VERSION=$(uname -r | cut -d. -f1-2)
echo "Kernel: $KERNEL_VERSION"

# Create virtual ethernet pair for testing
echo "Creating veth pair..."
ip link add veth-xdp0 type veth peer name veth-xdp1 2>/dev/null || true
ip link set veth-xdp0 up
ip link set veth-xdp1 up
ip addr add 10.0.0.1/24 dev veth-xdp0 2>/dev/null || true
ip addr add 10.0.0.2/24 dev veth-xdp1 2>/dev/null || true

echo "veth pair ready:"
echo "  veth-xdp0: 10.0.0.1"
echo "  veth-xdp1: 10.0.0.2"

# Build with XDP feature (requires nightly)
echo ""
echo "Building kaos-driver with XDP..."
cd "$(dirname "$0")/.."
rustup run nightly cargo build --release -p kaos-driver --features xdp

echo ""
echo "=== Ready to test ==="
echo "Terminal 1: ./target/release/kaos-driver 10.0.0.1:9000 10.0.0.2:9000"
echo "Terminal 2: ./target/release/kaos-driver 10.0.0.2:9000 10.0.0.1:9000"
echo ""
echo "Cleanup: ip link del veth-xdp0"

