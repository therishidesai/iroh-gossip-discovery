use dashmap::DashMap;

use iroh::{Endpoint, NodeId};
use iroh_gossip::{
    net::{Event, Gossip, GossipEvent, GossipReceiver, GossipSender},
    proto::TopicId,
};

use serde::{Deserialize, Serialize};

use std::sync::Arc;

use tokio::sync::{mpsc, mpsc::{UnboundedSender, UnboundedReceiver}};
use tokio::time::{sleep, Duration};



#[derive(Debug, Deserialize, Serialize)]
pub struct Node {
    pub name: String,
    pub node_id: NodeId,
    pub count: u32,
}


pub struct GossipDiscoveryBuilder {
    
}

impl GossipDiscoveryBuilder {
    pub async fn init(gossip: Gossip, seed_id: NodeId, topic_id: TopicId) -> Result<(GossipDiscoverySender, GossipDiscoveryReceiver)> {
	let (sender, mut receiver) = gossip
            .subscribe_and_join(id, vec![seed_id])
            .await?
            .split();
	
    }
}

pub struct GossipDiscoverySender {
    pub peer_rx: UnboundedReceiver<NodeId>,
    pub sender: GossipSender,
}

impl GossipDiscoverySender {
    pub async fn gossip(&mut self, node: Node, update_rate: Duration) {
	let mut i = node.count;

	loop {
	    match self.peers_rx.try_recv() {
                Ok(peer) => {
                    self.sender.join_peers(vec![peer]).await;
                }
                Err(_) => {}
            }

	    let update_node = Node {
		name: node.name,
		node_id: node.node_id,
		count: i,
	    };

	    let mut msg = Vec::new();
            ciborium::ser::into_writer(&update_node, &mut msg).expect("Serialization failed!");
            let bytes = bytes::Bytes::from(msg);
            let _ = self.sender.broadcast(bytes).await;
            i += 1;
            sleep(update_rate).await;
	}
    }
}

pub struct GossipDiscoveryReceiver {
    pub neighbor_map: Arc<DashMap<String, NodeId>>,
    pub peer_tx: UnbounededSender<NodeId>,
    pub receiver: GossipReceiver.
}

impl GossipDiscoveryReceiver {
    pub async fn update_map(&mut self) {
	while let Some(res) = self.receiver.next().await {
            if let Event::Gossip(GossipEvent::Received(msg)) = res? {
		let value: Node = ciborium::de::from_reader(&mut Cursor::new(msg.content))
                    .expect("Deserialization failed!");
		if !self.neighbor_map.contains_key(&value.name) {
		    // TODO: only send peers when we lose a neighbor so we dont broadcast as much
                    peers_tx.send(value.node_id.clone())?;
		}
		self.neighbor_map.insert(value.name, value.node_id);
                eprintln!("Address Book: \n {:?}", address_book);
            }
	}
    }
}
