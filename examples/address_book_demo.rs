use iroh::NodeId;
use iroh_gossip::{net::Gossip, proto::TopicId};
use iroh_gossip_discovery::{GossipDiscoveryBuilder, Node};
use std::env;
use std::str::FromStr;
use std::sync::Arc;
use tokio::time::{Duration, sleep};
use tracing::{info, error};
use tracing_subscriber;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing with immediate output
    tracing_subscriber::fmt()
        .with_target(false)
        .with_thread_ids(false)
        .with_line_number(false)
        .with_file(false)
        .compact()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "iroh_gossip_discovery=info".into())
        )
        .init();

    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <node_name> [seed_node_id]", args[0]);
        eprintln!("  Example: {} alice", args[0]);
        eprintln!("  Example: {} bob <alice_node_id>", args[0]);
        return Ok(());
    }

    let node_name = args[1].clone();
    let seed_node_id = if args.len() > 2 {
        Some(NodeId::from_str(&args[2])?)
    } else {
        None
    };

    // Create endpoint and gossip (with discovery enabled like the working example)
    let endpoint = iroh::Endpoint::builder()
        .discovery_n0()
        .discovery_local_network()
        .bind()
        .await?;
    info!(name = %node_name, node_id = %endpoint.node_id(), "Node started");

    let gossip = Gossip::builder().spawn(endpoint.clone()).await?;
    
    // Set up the router with gossip ALPN (required for gossip protocol)
    use iroh::protocol::Router;
    let _router = Router::builder(endpoint.clone())
        .accept(iroh_gossip::ALPN, gossip.clone())
        .spawn();
    
    let topic_id = TopicId::from([
        1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25,
        26, 27, 28, 29, 30, 31, 32,
    ]);

    // Initialize discovery with custom expiration timeout (60 seconds for demo)
    let (mut sender, mut receiver) = if let Some(seed_id) = seed_node_id {
        info!(%seed_id, "Connecting to seed node");
        GossipDiscoveryBuilder::new()
            .with_expiration_timeout(Duration::from_secs(60))
            .build_with_peers(gossip, topic_id, vec![seed_id], &endpoint)
            .await?
    } else {
        info!("Starting as first node - no existing peers to connect to");
        // For the first node, start with empty peer list - this will create the initial gossip network
        GossipDiscoveryBuilder::new()
            .with_expiration_timeout(Duration::from_secs(60))
            .build_with_peers(gossip, topic_id, vec![], &endpoint)
            .await?
    };

    let node = Node {
        name: node_name.clone(),
        node_id: endpoint.node_id(),
        count: 0,
    };

    // Get reference to neighbor map for periodic display
    let neighbor_map = Arc::clone(&receiver.neighbor_map);

    // Start receiver task
    let receiver_handle = tokio::spawn(async move {
        if let Err(e) = receiver.update_map().await {
            error!(%e, "Receiver error");
        }
    });

    // Start sender task
    let sender_node = node.clone();
    let sender_handle = tokio::spawn(async move {
        if let Err(e) = sender.gossip(sender_node, Duration::from_secs(3)).await {
            error!(%e, "Sender error");
        }
    });

    // Display address book periodically
    let display_handle = tokio::spawn(async move {
        let mut last_count = 0;
        loop {
            sleep(Duration::from_secs(5)).await;

            let neighbors: Vec<_> = neighbor_map
                .iter()
                .map(|entry| (entry.key().clone(), entry.value().node_id))
                .collect();

            let current_count = neighbors.len();
            if current_count != last_count || current_count == 0 {
                info!("\n📚 Address Book Update:");
                info!("   Self: {} ({})", &node_name, node.node_id);

                if neighbors.is_empty() {
                    info!("   👥 No peers discovered yet...");
                } else {
                    info!("   👥 Discovered peers ({}):", neighbors.len());
                    for (name, id) in &neighbors {
                        info!("      • {} ({})", name, id);
                    }
                }
                last_count = current_count;
            }
        }
    });

    // Keep running
    info!("\n🚀 Discovery system running... Press Ctrl+C to exit\n");

    // Wait for Ctrl+C
    tokio::signal::ctrl_c().await?;

    info!("\n🛑 Shutting down...");
    receiver_handle.abort();
    sender_handle.abort();
    display_handle.abort();

    Ok(())
}
