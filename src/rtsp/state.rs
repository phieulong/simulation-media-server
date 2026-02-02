use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Transport mode for RTP
#[derive(Clone, Debug, PartialEq)]
pub enum TransportMode {
    Udp {
        rtp_addr: SocketAddr,
        rtcp_addr: SocketAddr,
    },
    TcpInterleaved {
        rtp_channel: u8,
        rtcp_channel: u8,
    },
}

/// Client info sau khi SETUP
#[derive(Clone, Debug)]
pub struct ClientInfo {
    pub id: String,
    pub transport: TransportMode,
    pub is_playing: bool,
}

/// Shared state giữa RTSP sessions và streaming task
#[derive(Default)]
pub struct ServerState {
    pub clients: HashMap<String, ClientInfo>,
}

impl ServerState {
    pub fn new() -> Self {
        Self {
            clients: HashMap::new(),
        }
    }

    pub fn add_client(&mut self, info: ClientInfo) {
        println!("📝 Registered client: {} -> {:?}", info.id, info.transport);
        self.clients.insert(info.id.clone(), info);
    }

    pub fn set_playing(&mut self, session_id: &str, playing: bool) {
        if let Some(client) = self.clients.get_mut(session_id) {
            client.is_playing = playing;
            println!("▶️  Client {} is_playing = {}", session_id, playing);
        }
    }

    pub fn remove_client(&mut self, session_id: &str) {
        self.clients.remove(session_id);
        println!("🗑️  Removed client: {}", session_id);
    }

    pub fn get_playing_clients(&self) -> Vec<ClientInfo> {
        self.clients
            .values()
            .filter(|c| c.is_playing)
            .cloned()
            .collect()
    }

    pub fn get_udp_clients(&self) -> Vec<(SocketAddr, SocketAddr)> {
        self.clients
            .values()
            .filter(|c| c.is_playing)
            .filter_map(|c| {
                if let TransportMode::Udp { rtp_addr, rtcp_addr } = &c.transport {
                    Some((*rtp_addr, *rtcp_addr))
                } else {
                    None
                }
            })
            .collect()
    }
}

pub type SharedState = Arc<RwLock<ServerState>>;

pub fn create_shared_state() -> SharedState {
    Arc::new(RwLock::new(ServerState::new()))
}
