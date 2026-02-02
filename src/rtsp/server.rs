use tokio::net::TcpListener;
use super::session::RtspSession;

/// RTSP Server - xử lý control plane
pub struct RtspServer {
    addr: String,
}

impl RtspServer {
    pub fn new(addr: String) -> Self {
        Self { addr }
    }

    pub async fn run(&self) -> std::io::Result<()> {
        let listener = TcpListener::bind(&self.addr).await?;
        println!("🎥 RTSP Server listening on {}", self.addr);

        loop {
            let (socket, peer) = listener.accept().await?;
            println!("📡 Client connected: {}", peer);

            tokio::spawn(async move {
                let mut session = RtspSession::new(socket);
                if let Err(e) = session.handle().await {
                    eprintln!("❌ Session error: {}", e);
                }
            });
        }
    }
}
