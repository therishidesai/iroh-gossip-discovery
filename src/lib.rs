use bytes::Bytes;
use dashmap::DashMap;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use futures::StreamExt;

use iroh::NodeId;
use iroh_gossip::{
    net::{Event, Gossip, GossipEvent, GossipReceiver, GossipSender},
    proto::TopicId,
};

use serde::{Deserialize, Serialize};

use std::sync::Arc;
use thiserror::Error;

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::time::{Duration, Instant, sleep};
use tracing::{debug, error, info, warn};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Node {
    pub name: String,
    pub node_id: NodeId,
    pub count: u32,
}

#[derive(Debug, Clone)]
pub struct NodeInfo {
    pub node_id: NodeId,
    pub last_seen: Instant,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct SignedMessage {
    from: VerifyingKey,
    data: Bytes,
    signature: Signature,
}

impl SignedMessage {
    pub fn sign_and_encode(secret_key: &SigningKey, node: &Node) -> Result<Bytes> {
        let data: Bytes = postcard::to_stdvec(node)
            .map_err(|e| GossipDiscoveryError::Serialization(e.to_string()))?
            .into();
        let signature = secret_key.sign(&data);
        let from: VerifyingKey = secret_key.verifying_key();
        
        let signed_message = Self {
            from,
            data,
            signature,
        };
        
        let encoded = postcard::to_stdvec(&signed_message)
            .map_err(|e| GossipDiscoveryError::Serialization(e.to_string()))?;
        Ok(encoded.into())
    }
    
    pub fn verify_and_decode(bytes: &[u8]) -> Result<(VerifyingKey, Node)> {
        let signed_message: Self = postcard::from_bytes(bytes)
            .map_err(|e| GossipDiscoveryError::Deserialization(e.to_string()))?;
        let key: VerifyingKey = signed_message.from;
        
        key.verify(&signed_message.data, &signed_message.signature)
            .map_err(|e| GossipDiscoveryError::SignatureVerification(e.to_string()))?;
        
        let node: Node = postcard::from_bytes(&signed_message.data)
            .map_err(|e| GossipDiscoveryError::Deserialization(e.to_string()))?;
        Ok((signed_message.from, node))
    }
}

#[derive(Error, Debug)]
pub enum GossipDiscoveryError {
    #[error("Gossip error: {0}")]
    Gossip(#[from] iroh_gossip::net::Error),
    #[error("Channel send error")]
    ChannelSend,
    #[error("Serialization error: {0}")]
    Serialization(String),
    #[error("Deserialization error: {0}")]
    Deserialization(String),
    #[error("Signature verification error: {0}")]
    SignatureVerification(String),
    #[error("NodeId mismatch: expected {expected}, got {actual}")]
    NodeIdMismatch { expected: NodeId, actual: NodeId },
}

pub type Result<T> = std::result::Result<T, GossipDiscoveryError>;

pub struct GossipDiscoveryBuilder {
    expiration_timeout: Option<Duration>,
}

impl GossipDiscoveryBuilder {
    pub fn new() -> Self {
        Self {
            expiration_timeout: None,
        }
    }

    pub fn with_expiration_timeout(mut self, timeout: Duration) -> Self {
        self.expiration_timeout = Some(timeout);
        self
    }

    pub async fn build_with_peers(
        self,
        gossip: Gossip,
        topic_id: TopicId,
        peers: Vec<NodeId>,
        endpoint: &iroh::Endpoint,
    ) -> Result<(GossipDiscoverySender, GossipDiscoveryReceiver)> {
        // - First node (empty peers): use subscribe() only  
        // - Other nodes (with peers): use subscribe_and_join()
        info!("Attempting to subscribe to gossip topic");
        let (sender, receiver) = gossip.subscribe(topic_id, peers)?.split();
        info!("Subscribed to gossip topic");

        let (peer_tx, peer_rx) = tokio::sync::mpsc::unbounded_channel();
        let neighbor_map = Arc::new(DashMap::new());

        // Derive a secret key from the endpoint's node secret key
        // This ensures the signing key corresponds to the node's identity
        let node_secret = endpoint.secret_key();
        let secret_key_bytes = node_secret.to_bytes();
        let secret_key = SigningKey::from_bytes(&secret_key_bytes);
        let discovery_sender = GossipDiscoverySender { peer_rx, sender, secret_key };

        let expiration_timeout = self.expiration_timeout.unwrap_or(Duration::from_secs(30));

        let discovery_receiver = GossipDiscoveryReceiver {
            neighbor_map: Arc::clone(&neighbor_map),
            peer_tx,
            receiver,
            expiration_timeout,
        };

        // Start the cleanup task
        GossipDiscoveryReceiver::start_cleanup_task(neighbor_map, expiration_timeout);

        Ok((discovery_sender, discovery_receiver))
    }
}

pub struct GossipDiscoverySender {
    pub peer_rx: UnboundedReceiver<NodeId>,
    pub sender: GossipSender,
    pub secret_key: SigningKey,
}

impl GossipDiscoverySender {
    /// Add external peers to the gossip network
    pub async fn add_peers(&mut self, peers: Vec<NodeId>) -> Result<()> {
        if !peers.is_empty() {
            info!(peer_count = peers.len(), "Adding external peers to gossip network");
            self.sender.join_peers(peers).await?;
        }
        Ok(())
    }

    /// Add a single external peer to the gossip network  
    pub async fn add_peer(&mut self, peer: NodeId) -> Result<()> {
        self.add_peers(vec![peer]).await
    }

    pub async fn gossip(&mut self, node: Node, update_rate: Duration) -> Result<()> {
        let mut i = node.count;

        loop {
            // Check for new peers to join
            match self.peer_rx.try_recv() {
                Ok(peer) => {
                    info!(%peer, "Joining new peer");
                    if let Err(e) = self.sender.join_peers(vec![peer]).await {
                        error!(%e, "Failed to join peer");
                    }
                }
                Err(_) => {}
            }

            let update_node = Node {
                name: node.name.clone(),
                node_id: node.node_id,
                count: i,
            };

            // Sign and encode the message
            let bytes = SignedMessage::sign_and_encode(&self.secret_key, &update_node)?;

            if let Err(e) = self.sender.broadcast(bytes).await {
                error!(%e, "Failed to broadcast");
            }

            i += 1;
            sleep(update_rate).await;
        }
    }
}

pub struct GossipDiscoveryReceiver {
    pub neighbor_map: Arc<DashMap<String, NodeInfo>>,
    pub peer_tx: UnboundedSender<NodeId>,
    pub receiver: GossipReceiver,
    pub expiration_timeout: Duration,
}

impl GossipDiscoveryReceiver {
    pub async fn update_map(&mut self) -> Result<()> {
        while let Some(res) = self.receiver.next().await {
            match res {
                Ok(Event::Gossip(GossipEvent::Received(msg))) => {
                    // Verify and decode the signed message
                    let (verifying_key, value) = match SignedMessage::verify_and_decode(&msg.content) {
                        Ok(result) => result,
                        Err(e) => {
                            warn!(%e, "Failed to verify message signature, ignoring");
                            continue;
                        }
                    };

                    // Verify that the claimed node_id matches the public key
                    let expected_node_id = NodeId::from(verifying_key);
                    if value.node_id != expected_node_id {
                        warn!(
                            claimed_node_id = %value.node_id,
                            actual_node_id = %expected_node_id,
                            "NodeId spoofing attempt detected, ignoring message"
                        );
                        continue;
                    }

                    let is_new_peer = !self.neighbor_map.contains_key(&value.name);

                    if is_new_peer {
                        // Send new peer to sender for joining
                        self.peer_tx
                            .send(value.node_id)
                            .map_err(|_| GossipDiscoveryError::ChannelSend)?;
                        info!(name = %value.name, node_id = %value.node_id, "Discovered new peer");
                    }

                    self.neighbor_map.insert(
                        value.name.clone(),
                        NodeInfo {
                            node_id: value.node_id,
                            last_seen: Instant::now(),
                        },
                    );
                    debug!(peer_count = self.neighbor_map.len(), "Address book updated");
                }
                Ok(_) => {}
                Err(e) => {
                    error!(%e, "Error receiving gossip");
                }
            }
        }
        Ok(())
    }

    pub fn get_neighbors(&self) -> Vec<(String, NodeId)> {
        self.neighbor_map
            .iter()
            .map(|entry| (entry.key().clone(), entry.value().node_id))
            .collect()
    }

    pub fn cleanup_expired_nodes(&self) -> usize {
        let now = Instant::now();
        let mut expired_count = 0;

        // Collect expired node names first to avoid holding locks
        let expired_nodes: Vec<String> = self
            .neighbor_map
            .iter()
            .filter_map(|entry| {
                if now.duration_since(entry.value().last_seen) > self.expiration_timeout {
                    Some(entry.key().clone())
                } else {
                    None
                }
            })
            .collect();

        // Remove expired nodes
        for node_name in expired_nodes {
            if let Some((_, node_info)) = self.neighbor_map.remove(&node_name) {
                info!(name = %node_name, node_id = %node_info.node_id, "Expired node");
                expired_count += 1;
            }
        }

        expired_count
    }

    pub fn start_cleanup_task(
        neighbor_map: Arc<DashMap<String, NodeInfo>>,
        expiration_timeout: Duration,
    ) {
        let cleanup_interval = expiration_timeout / 3; // Check every 1/3 of timeout period

        tokio::spawn(async move {
            loop {
                sleep(cleanup_interval).await;

                let now = Instant::now();
                let mut expired_count = 0;

                // Collect expired node names first to avoid holding locks
                let expired_nodes: Vec<String> = neighbor_map
                    .iter()
                    .filter_map(|entry| {
                        if now.duration_since(entry.value().last_seen) > expiration_timeout {
                            Some(entry.key().clone())
                        } else {
                            None
                        }
                    })
                    .collect();

                // Remove expired nodes
                for node_name in expired_nodes {
                    if let Some((_, node_info)) = neighbor_map.remove(&node_name) {
                        info!(name = %node_name, node_id = %node_info.node_id, "Expired node");
                        expired_count += 1;
                    }
                }

                if expired_count > 0 {
                    info!(count = expired_count, "Cleaned up expired nodes");
                }
            }
        });
    }
}
