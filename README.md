# Iroh Gossip Discovery

A Rust library implementing gossip-based peer discovery for peer-to-peer networks using the [Iroh](https://iroh.computer/) ecosystem. This library provides automatic peer discovery and address book management through gossip protocols, enabling resilient P2P network formation.

## Features

- **Automatic Peer Discovery**: Nodes automatically discover and connect to peers in the network
- **Gossip-Based Communication**: Uses Iroh's gossip protocol for efficient message propagation
- **Address Book Management**: Maintains an up-to-date view of all discovered peers
- **Node Expiration**: Automatically removes inactive nodes from the address book
- **Network Resilience**: Handles peer disconnections and network partitions gracefully
- **Structured Logging**: Uses `tracing` for comprehensive observability

## Architecture

The library consists of three main components:

- **`GossipDiscoveryBuilder`**: Factory for creating gossip discovery instances with configurable options
- **`GossipDiscoverySender`**: Handles broadcasting node information and managing peer connections
- **`GossipDiscoveryReceiver`**: Processes incoming gossip messages and maintains the neighbor map

### Core Data Structures

- **`Node`**: Represents a network node with name, ID, and counter
- **`NodeInfo`**: Internal structure tracking node information with timestamps for expiration

### Gossip Protocol Integration

The library integrates with Iroh's gossip protocol by:

1. **Topic-based Communication**: All discovery messages use a shared topic ID
2. **Automatic Peer Joining**: New peers are automatically added to gossip subscriptions
3. **Message Serialization**: Uses CBOR for efficient message encoding
4. **Network Discovery**: Leverages Iroh's local network discovery mechanisms

### Node Lifecycle Management

- **Discovery**: Nodes broadcast their presence periodically
- **Connection**: Automatic peer joining when new nodes are discovered  
- **Monitoring**: Continuous health checking with configurable timeouts
- **Cleanup**: Automatic removal of inactive nodes to prevent memory leaks

## Examples

### Address Book Demo

The library includes a comprehensive example demonstrating peer discovery:

```bash
# Start first node (seed)
cargo run --example address_book_demo alice

# Connect additional nodes (use alice's node ID from output)
cargo run --example address_book_demo bob <alice_node_id>
cargo run --example address_book_demo charlie <alice_node_id>
```

## Development

### Requirements

This project uses [Nix](https://nixos.org/) for reproducible development environments:

```bash
# Enter development shell
nix develop

# Build the project
cargo build

# Check code formatting
cargo fmt

# Run linter
cargo clippy
```

### Testing Network Convergence

The project includes automated testing for network convergence:

```bash
# Test with default settings (5 nodes, 30 seconds)
nix run .#test-network

# Test with custom parameters
nix run .#test-network 10 60  # 10 nodes, 60 seconds
```

The test script will:
1. Start multiple nodes in sequence
2. Monitor peer discovery progress
3. Verify all nodes converge to the same address book
4. Report success/failure with detailed analysis

