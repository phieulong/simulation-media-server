use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use super::state::{SharedState, ClientInfo, TransportMode};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::Mutex;

/// RTSP Session - xử lý các request từ 1 client
pub struct RtspSession {
    socket: Arc<Mutex<TcpStream>>,
    cseq: u32,
    session_id: String,
    client_ip: String,
    rtp_port: Option<u16>,
    rtcp_port: Option<u16>,
    transport_mode: Option<TransportMode>,
    state: SharedState,
}

impl RtspSession {
    pub fn new(socket: TcpStream, state: SharedState) -> Self {
        let client_ip = socket
            .peer_addr()
            .map(|a| a.ip().to_string())
            .unwrap_or_else(|_| "127.0.0.1".to_string());

        Self {
            socket: Arc::new(Mutex::new(socket)),
            cseq: 0,
            session_id: Self::generate_session_id(),
            client_ip,
            rtp_port: None,
            rtcp_port: None,
            transport_mode: None,
            state,
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

    /// Get socket for TCP interleaved streaming
    pub fn get_socket(&self) -> Arc<Mutex<TcpStream>> {
        self.socket.clone()
    }

    /// Handle RTSP requests
    pub async fn handle(&mut self) -> std::io::Result<()> {
        let mut buffer = vec![0u8; 4096];

        loop {
            let n = {
                let mut sock = self.socket.lock().await;
                sock.read(&mut buffer).await?
            };

            if n == 0 {
                println!("🔌 Client disconnected");
                self.state.write().await.remove_client(&self.session_id);
                break;
            }

            let request = String::from_utf8_lossy(&buffer[..n]);
            println!("📥 Request:\n{}", request);

            let response = self.process_request(&request).await;

            {
                let mut sock = self.socket.lock().await;
                sock.write_all(response.as_bytes()).await?;
                sock.flush().await?;
            }

            println!("📤 Response sent\n");

            // If PLAY was called and we're using TCP interleaved, start streaming on this connection
            if let Some(TransportMode::TcpInterleaved { rtp_channel, rtcp_channel }) = &self.transport_mode {
                let state = self.state.read().await;
                if let Some(client) = state.clients.get(&self.session_id) {
                    if client.is_playing {
                        drop(state);
                        // Start TCP interleaved streaming
                        self.start_tcp_streaming(*rtp_channel, *rtcp_channel).await?;
                    }
                }
            }
        }

        Ok(())
    }

    async fn start_tcp_streaming(&self, rtp_channel: u8, _rtcp_channel: u8) -> std::io::Result<()> {
        use crate::source::file::{FileSource, NaluParser};
        use crate::rtp::h264::H264Packetizer;
        use std::io::Read;
        use std::time::Duration;
        use tokio::time::Instant;

        println!("🎬 Starting TCP interleaved streaming on channel {}", rtp_channel);

        let video_path = "./videos/example.mp4";
        if !std::path::Path::new(video_path).exists() {
            eprintln!("⚠️  Video file not found for TCP streaming");
            return Ok(());
        }

        let source = FileSource::new(video_path.to_string());
        let mut child = source.start_ffmpeg()?;

        let stdout = child.stdout.take().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::Other, "Failed to capture FFmpeg stdout")
        })?;

        let mut parser = NaluParser::new();
        let mut packetizer = H264Packetizer::new(0x12345678);
        let mut reader = std::io::BufReader::new(stdout);
        let mut buffer = [0u8; 8192];

        // Timing control
        let start_time = Instant::now();
        let mut frame_count: u64 = 0;
        let frame_duration = Duration::from_micros(33333); // ~30fps

        // Store SPS/PPS for re-sending before IDR
        let mut cached_sps: Option<Vec<u8>> = None;
        let mut cached_pps: Option<Vec<u8>> = None;

        loop {
            // Check if client is still playing
            {
                let state = self.state.read().await;
                if let Some(client) = state.clients.get(&self.session_id) {
                    if !client.is_playing {
                        println!("⏹️  Client stopped playing, ending TCP stream");
                        break;
                    }
                } else {
                    println!("⏹️  Client disconnected, ending TCP stream");
                    break;
                }
            }

            match reader.read(&mut buffer) {
                Ok(0) => {
                    println!("📹 FFmpeg stream ended");
                    break;
                }
                Ok(n) => {
                    let nalus = parser.parse(&buffer[..n]);

                    for nalu in nalus.iter() {
                        if nalu.is_empty() {
                            continue;
                        }

                        let nalu_type = nalu[0] & 0x1F;

                        // Cache SPS/PPS
                        match nalu_type {
                            7 => { // SPS
                                cached_sps = Some(nalu.clone());
                                println!("📋 Cached SPS ({} bytes)", nalu.len());
                            }
                            8 => { // PPS
                                cached_pps = Some(nalu.clone());
                                println!("📋 Cached PPS ({} bytes)", nalu.len());
                            }
                            5 => { // IDR - send SPS/PPS first
                                // Send cached SPS before IDR
                                if let Some(ref sps) = cached_sps {
                                    let packets = packetizer.packetize(sps, false);
                                    for packet in packets {
                                        self.send_interleaved_rtp(&packet.to_bytes(), rtp_channel).await?;
                                    }
                                }
                                // Send cached PPS before IDR
                                if let Some(ref pps) = cached_pps {
                                    let packets = packetizer.packetize(pps, false);
                                    for packet in packets {
                                        self.send_interleaved_rtp(&packet.to_bytes(), rtp_channel).await?;
                                    }
                                }
                            }
                            _ => {}
                        }

                        // Determine if this is last NALU of current Access Unit
                        // For simplicity, treat each NALU with type 1-5 as end of AU
                        let is_au_end = nalu_type >= 1 && nalu_type <= 5;

                        let packets = packetizer.packetize(nalu, is_au_end);

                        for packet in packets {
                            self.send_interleaved_rtp(&packet.to_bytes(), rtp_channel).await?;
                        }

                        // Increment timestamp after each Access Unit (frame)
                        if is_au_end {
                            frame_count += 1;
                            packetizer.increment_timestamp(3000); // 90000/30 = 3000

                            // Timing control - wait until next frame time
                            let expected_time = start_time + frame_duration * frame_count as u32;
                            let now = Instant::now();
                            if expected_time > now {
                                tokio::time::sleep(expected_time - now).await;
                            }

                            if frame_count % 30 == 0 {
                                println!("🎬 TCP: Sent {} frames", frame_count);
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("❌ Read error: {}", e);
                    break;
                }
            }
        }

        let _ = child.kill();
        Ok(())
    }

    async fn send_interleaved_rtp(&self, rtp_data: &[u8], channel: u8) -> std::io::Result<()> {
        // TCP interleaved format: $<channel><length_high><length_low><data>
        let mut interleaved = Vec::with_capacity(4 + rtp_data.len());
        interleaved.push(b'$');
        interleaved.push(channel);
        interleaved.push((rtp_data.len() >> 8) as u8);
        interleaved.push((rtp_data.len() & 0xFF) as u8);
        interleaved.extend_from_slice(rtp_data);

        let mut sock = self.socket.lock().await;
        sock.write_all(&interleaved).await
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
            "SETUP" => self.handle_setup(request).await,
            "PLAY" => self.handle_play().await,
            "TEARDOWN" => self.handle_teardown().await,
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
        // SPS/PPS cho 640x480 baseline profile
        let sps_base64 = "Z0IAH6tAUB7I";
        let pps_base64 = "aM4wpIA=";

        let sdp = format!(
            "v=0\r\n\
             o=- 0 0 IN IP4 127.0.0.1\r\n\
             s=Simulation Media Server\r\n\
             c=IN IP4 0.0.0.0\r\n\
             t=0 0\r\n\
             m=video 0 RTP/AVP 96\r\n\
             a=rtpmap:96 H264/90000\r\n\
             a=fmtp:96 packetization-mode=1;profile-level-id=42001f;sprop-parameter-sets={},{}\r\n\
             a=control:track1\r\n",
            sps_base64, pps_base64
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

    async fn handle_setup(&mut self, request: &str) -> String {
        // Parse Transport header
        let mut is_tcp = false;
        let mut interleaved_rtp: u8 = 0;
        let mut interleaved_rtcp: u8 = 1;
        let mut client_rtp_port: u16 = 5004;
        let mut client_rtcp_port: u16 = 5005;

        for line in request.lines() {
            if line.starts_with("Transport:") {
                let transport_value = &line["Transport:".len()..];
                println!("📋 Transport header: {}", transport_value);

                // Check if TCP interleaved
                if transport_value.contains("TCP") || transport_value.contains("interleaved") {
                    is_tcp = true;

                    // Parse interleaved=x-y
                    for part in transport_value.split(';') {
                        let part = part.trim();
                        if part.starts_with("interleaved=") {
                            if let Some(channels) = part.strip_prefix("interleaved=") {
                                let channel_parts: Vec<&str> = channels.split('-').collect();
                                if !channel_parts.is_empty() {
                                    interleaved_rtp = channel_parts[0].parse().unwrap_or(0);
                                    if channel_parts.len() >= 2 {
                                        interleaved_rtcp = channel_parts[1].parse().unwrap_or(1);
                                    } else {
                                        interleaved_rtcp = interleaved_rtp + 1;
                                    }
                                }
                            }
                        }
                    }
                } else {
                    // UDP mode - parse client_port
                    for part in transport_value.split(';') {
                        let part = part.trim();
                        if part.starts_with("client_port=") {
                            if let Some(ports) = part.strip_prefix("client_port=") {
                                let port_parts: Vec<&str> = ports.split('-').collect();
                                if !port_parts.is_empty() {
                                    client_rtp_port = port_parts[0].parse().unwrap_or(5004);
                                    if port_parts.len() >= 2 {
                                        client_rtcp_port = port_parts[1].parse().unwrap_or(client_rtp_port + 1);
                                    } else {
                                        client_rtcp_port = client_rtp_port + 1;
                                    }
                                }
                            }
                        }
                    }
                }
                break;
            }
        }

        let (transport_mode, transport_response) = if is_tcp {
            println!("🔌 TCP interleaved mode: channels {}-{}", interleaved_rtp, interleaved_rtcp);

            let mode = TransportMode::TcpInterleaved {
                rtp_channel: interleaved_rtp,
                rtcp_channel: interleaved_rtcp,
            };

            let response = format!(
                "RTP/AVP/TCP;unicast;interleaved={}-{}",
                interleaved_rtp, interleaved_rtcp
            );

            (mode, response)
        } else {
            println!("📡 UDP mode: client ports {}-{}", client_rtp_port, client_rtcp_port);

            self.rtp_port = Some(client_rtp_port);
            self.rtcp_port = Some(client_rtcp_port);

            let rtp_addr: SocketAddr = format!("{}:{}", self.client_ip, client_rtp_port)
                .parse()
                .unwrap_or_else(|_| "127.0.0.1:5004".parse().unwrap());
            let rtcp_addr: SocketAddr = format!("{}:{}", self.client_ip, client_rtcp_port)
                .parse()
                .unwrap_or_else(|_| "127.0.0.1:5005".parse().unwrap());

            let mode = TransportMode::Udp { rtp_addr, rtcp_addr };

            let response = format!(
                "RTP/AVP;unicast;client_port={}-{};server_port=6000-6001",
                client_rtp_port, client_rtcp_port
            );

            (mode, response)
        };

        self.transport_mode = Some(transport_mode.clone());

        let client_info = ClientInfo {
            id: self.session_id.clone(),
            transport: transport_mode,
            is_playing: false,
        };

        self.state.write().await.add_client(client_info);

        format!(
            "RTSP/1.0 200 OK\r\n\
             CSeq: {}\r\n\
             Session: {}\r\n\
             Transport: {}\r\n\
             \r\n",
            self.cseq,
            self.session_id,
            transport_response
        )
    }

    async fn handle_play(&mut self) -> String {
        self.state.write().await.set_playing(&self.session_id, true);

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

    async fn handle_teardown(&self) -> String {
        self.state.write().await.remove_client(&self.session_id);

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
