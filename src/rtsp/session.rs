use tokio::net::{TcpStream, UdpSocket};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use std::sync::Arc;
use tokio::sync::Mutex;
use std::time::Duration;

/// RTSP Session - xử lý các request từ 1 client
pub struct RtspSession {
    socket: TcpStream,
    cseq: u32,
    session_id: String,
    rtp_port: Option<u16>,
    rtcp_port: Option<u16>,
}

impl RtspSession {
    pub fn new(socket: TcpStream) -> Self {
        Self {
            socket,
            cseq: 0,
            session_id: Self::generate_session_id(),
            rtp_port: None,
            rtcp_port: None,
        }
    }

    fn generate_session_id() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        format!("{:x}", timestamp)
    }

    /// Handle RTSP requests
    pub async fn handle(&mut self) -> std::io::Result<()> {
        let mut buffer = vec![0u8; 4096];
        
        loop {
            let n = self.socket.read(&mut buffer).await?;
            if n == 0 {
                println!("🔌 Client disconnected");
                break;
            }

            let request = String::from_utf8_lossy(&buffer[..n]);
            println!("📥 Request:\n{}", request);

            let response = self.process_request(&request).await;
            
            self.socket.write_all(response.as_bytes()).await?;
            self.socket.flush().await?;
            
            println!("📤 Response sent\n");
        }

        Ok(())
    }

    async fn process_request(&mut self, request: &str) -> String {
        let lines: Vec<&str> = request.lines().collect();
        if lines.is_empty() {
            return self.error_response(400, "Bad Request");
        }

        let request_line = lines[0];
        let parts: Vec<&str> = request_line.split_whitespace().collect();
        
        if parts.len() < 2 {
            return self.error_response(400, "Bad Request");
        }

        let method = parts[0];
        let _url = parts[1];

        // Parse CSeq
        for line in &lines {
            if line.starts_with("CSeq:") {
                if let Some(cseq_str) = line.split(':').nth(1) {
                    self.cseq = cseq_str.trim().parse().unwrap_or(0);
                }
            }
        }

        match method {
            "OPTIONS" => self.handle_options(),
            "DESCRIBE" => self.handle_describe(),
            "SETUP" => self.handle_setup(request),
            "PLAY" => self.handle_play(),
            "TEARDOWN" => self.handle_teardown(),
            _ => self.error_response(405, "Method Not Allowed"),
        }
    }

    fn handle_options(&self) -> String {
        format!(
            "RTSP/1.0 200 OK\r\n\
             CSeq: {}\r\n\
             Public: OPTIONS, DESCRIBE, SETUP, PLAY, TEARDOWN\r\n\
             \r\n",
            self.cseq
        )
    }

    fn handle_describe(&self) -> String {
        let sdp = format!(
            "v=0\r\n\
             o=- 0 0 IN IP4 127.0.0.1\r\n\
             s=Simulation Media Server\r\n\
             t=0 0\r\n\
             m=video 0 RTP/AVP 96\r\n\
             a=rtpmap:96 H264/90000\r\n\
             a=fmtp:96 packetization-mode=1\r\n\
             a=control:track1\r\n"
        );

        format!(
            "RTSP/1.0 200 OK\r\n\
             CSeq: {}\r\n\
             Content-Type: application/sdp\r\n\
             Content-Length: {}\r\n\
             \r\n\
             {}",
            self.cseq,
            sdp.len(),
            sdp
        )
    }

    fn handle_setup(&mut self, request: &str) -> String {
        // Parse Transport header để lấy client_port
        let mut client_rtp_port = 5004;
        let mut client_rtcp_port = 5005;
        
        for line in request.lines() {
            if line.starts_with("Transport:") {
                if let Some(transport) = line.split(':').nth(1) {
                    // Parse client_port=xxxx-yyyy
                    for part in transport.split(';') {
                        if part.trim().starts_with("client_port=") {
                            if let Some(ports) = part.trim().strip_prefix("client_port=") {
                                let port_parts: Vec<&str> = ports.split('-').collect();
                                if port_parts.len() == 2 {
                                    client_rtp_port = port_parts[0].parse().unwrap_or(5004);
                                    client_rtcp_port = port_parts[1].parse().unwrap_or(5005);
                                }
                            }
                        }
                    }
                }
            }
        }

        self.rtp_port = Some(client_rtp_port);
        self.rtcp_port = Some(client_rtcp_port);

        let server_rtp_port = 6000;
        let server_rtcp_port = 6001;

        format!(
            "RTSP/1.0 200 OK\r\n\
             CSeq: {}\r\n\
             Session: {}\r\n\
             Transport: RTP/AVP;unicast;client_port={}-{};server_port={}-{}\r\n\
             \r\n",
            self.cseq,
            self.session_id,
            client_rtp_port,
            client_rtcp_port,
            server_rtp_port,
            server_rtcp_port
        )
    }

    fn handle_play(&self) -> String {
        format!(
            "RTSP/1.0 200 OK\r\n\
             CSeq: {}\r\n\
             Session: {}\r\n\
             RTP-Info: url=rtsp://127.0.0.1:8554/cam/track1;seq=0;rtptime=0\r\n\
             \r\n",
            self.cseq,
            self.session_id
        )
    }

    fn handle_teardown(&self) -> String {
        format!(
            "RTSP/1.0 200 OK\r\n\
             CSeq: {}\r\n\
             Session: {}\r\n\
             \r\n",
            self.cseq,
            self.session_id
        )
    }

    fn error_response(&self, code: u16, reason: &str) -> String {
        format!(
            "RTSP/1.0 {} {}\r\n\
             CSeq: {}\r\n\
             \r\n",
            code, reason, self.cseq
        )
    }
}
