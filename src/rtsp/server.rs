use tokio::net::TcpListener;
use super::session::RtspSession;
use super::state::SharedState;

/// RTSP Server - xử lý control plane
pub struct RtspServer {
    addr: String,
    state: SharedState,
}

impl RtspServer {
    pub fn new(addr: String, state: SharedState) -> Self {
        Self { addr, state }
    }

    pub async fn run(&self) -> std::io::Result<()> {
        let listener = TcpListener::bind(&self.addr).await?;
        println!("🎥 RTSP Server listening on {}", self.addr);

        loop {
            let (socket, peer) = listener.accept().await?;
            println!("📡 Client connected: {}", peer);

            let state = self.state.clone();
            tokio::spawn(async move {
                let mut session = RtspSession::new(socket, state);
                if let Err(e) = session.handle().await {
                    eprintln!("❌ Session error: {}", e);
                }
            });
        }
    }
}
