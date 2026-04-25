use futures::StreamExt;
use iroh::{Endpoint, EndpointId, RelayMode, SecretKey, address_lookup::DnsAddressLookup, dns::DnsResolver};
use iroh_gossip::{net::Gossip, proto::TopicId};
use iroh_gossip_discovery::{GossipDiscoveryBuilder, Node};
use std::env;
use std::str::FromStr;
use std::sync::Arc;
use tokio::{sync::Mutex, time::{Duration, sleep}};
use tracing::{error, info, trace};
use tracing_subscriber;

#[derive(Debug, Copy, Clone, Default)]
pub struct LocalDiscoveryPreset;

impl iroh::endpoint::presets::Preset for LocalDiscoveryPreset {
    fn apply(
        self,
        mut builder: iroh::endpoint::Builder,
    ) -> iroh::endpoint::Builder {

        builder = builder.crypto_provider(Arc::new(rustls::crypto::ring::default_provider()));
        
        use iroh::RelayMode;
        builder = builder.relay_mode(RelayMode::Disabled);

        use iroh::address_lookup::MdnsAddressLookup;
        builder = builder.address_lookup(MdnsAddressLookup::builder());


        builder
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_target(false)
        .with_thread_ids(false)
        .with_line_number(false)
        .with_file(false)
        .compact()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "iroh_gossip_discovery=info,address_book_demo=info".into())
        )
        .init();

    // Parse args
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <node_name> [seed_node_id]", args[0]);
        eprintln!("  Example: {} alice", args[0]);
        eprintln!("  Example: {} bob <alice_node_id>", args[0]);
        return Ok(());
    }

    let node_name = args[1].clone();
    let seed_node_id = if args.len() > 2 {
        Some(EndpointId::from_str(&args[2])?)
    } else {
        None
    };

    // Endpoint
    let secret_key = SecretKey::generate();
    let endpoint = Endpoint::builder(LocalDiscoveryPreset)
        .secret_key(secret_key.clone())
        .relay_mode(RelayMode::Disabled)
        .bind()
        .await?;

    // mDNS
    let mdns = iroh::address_lookup::mdns::MdnsAddressLookup::builder()
        .advertise(true)
        .build(endpoint.id())
        .unwrap();

    endpoint.address_lookup()?.add(mdns.clone());

    info!(name = %node_name, node_id = %endpoint.id(), "Node started");

    // Gossip
    let gossip = Gossip::builder().spawn(endpoint.clone());

    // Router
    use iroh::protocol::Router;
    let _router = Router::builder(endpoint.clone())
        .accept(iroh_gossip::ALPN, gossip.clone())
        .spawn();

    // Topic
    let topic_id = TopicId::from([1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31,32]);

    // -------------------------------
    // DISCOVERY INITIALIZATION
    // -------------------------------
    let (mut sender, mut receiver) = GossipDiscoveryBuilder::new()
        .with_expiration_timeout(Duration::from_secs(60))
        .build_with_peers(
            gossip.clone(),
            topic_id,
            seed_node_id.into_iter().collect(),
            &endpoint,
        )
        .await?;

    // -------------------------------
    // CHANNEL for add_peer()
    // -------------------------------
    let (peer_tx, mut peer_rx) = tokio::sync::mpsc::unbounded_channel::<EndpointId>();

    // -------------------------------
    // mDNS → send peers in channel
    // -------------------------------
    let mdns_clone = mdns.clone();
    let neighbor_map_for_mdns = receiver.neighbor_map.clone();
    let peer_tx_mdns = peer_tx.clone();

    tokio::spawn(async move {
        let mut events = mdns_clone.subscribe().await;

        while let Some(event) = events.next().await {
            match event {
                iroh::address_lookup::DiscoveryEvent::Discovered { endpoint_info, .. } => {
                    trace!("[MDNS] discovered: {:?}", endpoint_info);

                    // check if peer already known
                    let already_known = neighbor_map_for_mdns
                        .iter()
                        .any(|entry| entry.value().node_id == endpoint_info.endpoint_id);

                    if already_known {
                        trace!("[MDNS] peer {} already known, skipping", endpoint_info.endpoint_id);
                        continue;
                    }

                    // Peer in Channel senden
                    let _ = peer_tx_mdns.send(endpoint_info.endpoint_id);
                }
                iroh::address_lookup::DiscoveryEvent::Expired { endpoint_id } => {
                    trace!("[MDNS] expired: {endpoint_id}");
                }
                _ => {}
            }
        }
    });

    // -------------------------------
    // DISCOVERY LOOP
    // -------------------------------
    let node = Node {
        name: node_name.clone(),
        node_id: endpoint.id(),
        count: 0,
    };

    let sender_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                Some(peer) = peer_rx.recv() => {
                    if let Err(e) = sender.add_peer(peer).await {
                        eprintln!("add_peer failed: {e}");
                    }
                }

                result = sender.gossip(node.clone(), Duration::from_secs(3)) => {
                    if let Err(e) = result {
                        eprintln!("gossip error: {e}");
                    }
                    break;
                }
            }
        }
    });

    // -------------------------------
    // RECEIVER LOOP
    // -------------------------------
    let neighbor_map = Arc::clone(&receiver.neighbor_map);

    let receiver_handle = tokio::spawn(async move {
        if let Err(e) = receiver.update_map().await {
            error!(%e, "Receiver error");
        }
    });

    // -------------------------------
    // DISPLAY LOOP
    // -------------------------------
    let display_handle = {
        let node_name = node_name.clone();
        let node_id = endpoint.id();

        tokio::spawn(async move {
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
                    info!("   Self: {} ({})", &node_name, node_id);

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
        })
    };

    // -------------------------------
    // SHUTDOWN
    // -------------------------------
    info!("\n🚀 Discovery system running... Press Ctrl+C to exit\n");

    tokio::signal::ctrl_c().await?;

    info!("\n🛑 Shutting down...");
    receiver_handle.abort();
    display_handle.abort();
    sender_task.abort();

    Ok(())
}
