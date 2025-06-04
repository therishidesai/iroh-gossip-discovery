# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Development Commands

**IMPORTANT**: All cargo commands must be run within the Nix development shell:
```bash
nix develop
```

Once in the dev shell:
- **Build**: `cargo build`
- **Run tests**: `cargo test`
- **Check code**: `cargo check`
- **Format code**: `cargo fmt`
- **Lint code**: `cargo clippy`

## Architecture Overview

This is a Rust library implementing gossip-based discovery for peer-to-peer networks using the Iroh ecosystem. The core architecture consists of:

### Core Components

- **GossipDiscoveryBuilder**: Factory for creating gossip discovery instances
- **GossipDiscoverySender**: Handles broadcasting node information and managing peer connections
- **GossipDiscoveryReceiver**: Processes incoming gossip messages and maintains a neighbor map
- **Node**: Data structure representing a network node with name, ID, and counter

### Key Dependencies

- `iroh` and `iroh-gossip`: Core networking and gossip protocol implementation
- `dashmap`: Thread-safe concurrent hash map for neighbor tracking
- `ciborium`: CBOR serialization for message encoding
- `tokio`: Async runtime with full features

### Development Environment

The project uses Nix flakes for reproducible development environments with:
- Rust nightly toolchain
- Required system dependencies (openssl, pkg-config)
- rust-analyzer for IDE support

## Usage Examples

### Running the Address Book Demo

```bash
# Terminal 1 - Start seed node
nix develop
cargo run --example address_book_demo alice

# Terminal 2 - Connect second node (use alice's node ID from terminal 1)
nix develop
cargo run --example address_book_demo bob <alice_node_id>

# Terminal 3 - Connect third node
nix develop
cargo run --example address_book_demo charlie <alice_node_id>
```

### Library API

```rust
use iroh_gossip_discovery::{GossipDiscoveryBuilder, Node};

// Create discovery system
let (mut sender, mut receiver) = GossipDiscoveryBuilder::init(
    gossip, 
    topic_id, 
    seed_node_id
).await?;

// Start broadcasting your presence
let node = Node { name: "alice".to_string(), node_id, count: 0 };
tokio::spawn(async move {
    sender.gossip(node, Duration::from_secs(5)).await
});

// Monitor discovered peers
tokio::spawn(async move {
    receiver.update_map().await
});

// Access the address book
let neighbors = receiver.get_neighbors();
```

## Architecture Notes

The library automatically handles peer joining via `join_peers()` when new nodes are discovered, ensuring network resilience when peers disconnect.