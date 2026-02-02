mod source;
mod rtsp;
mod rtp;
mod rtcp;

use std::env;
use rtsp::server::RtspServer;
use rtsp::state::{SharedState, create_shared_state};
use rtp::h264::H264Packetizer;
use rtcp::sr::SenderReport;
use source::file::{FileSource, NaluParser};
use tokio::net::UdpSocket;
use std::sync::Arc;
use tokio::sync::Mutex;
use std::time::Duration;
use std::io::Read;

#[tokio::main]
async fn main() {
    println!("🚀 Simulation Media Server Starting...");
    println!("=====================================");
    
    // Create shared state
    let state = create_shared_state();

    // Start RTSP server
    let rtsp_server = RtspServer::new("0.0.0.0:8554".to_string(), state.clone());

    let rtsp_handle = tokio::spawn(async move {
        if let Err(e) = rtsp_server.run().await {
            eprintln!("❌ RTSP Server error: {}", e);
        }
    });

    println!("Application run on: {} ", env::current_dir().unwrap().display());

    // Start RTP/RTCP streaming task
    let streaming_state = state.clone();
    let streaming_handle = tokio::spawn(async move {
        // Đợi một chút để RTSP server khởi động
        tokio::time::sleep(Duration::from_secs(2)).await;

        println!("\n📹 Starting video streaming...");
        println!("=====================================");
        
        // Khởi động video source
        if let Err(e) = start_video_streaming(streaming_state).await {
            eprintln!("❌ Video streaming error: {}", e);
        }
    });

    // Wait for both tasks
    let _ = tokio::join!(rtsp_handle, streaming_handle);
}

/// Start video streaming từ MP4 file
async fn start_video_streaming(state: SharedState) -> std::io::Result<()> {
    let video_path = "./videos/example.mp4";

    println!("Debug: requested video_path = {:?}", video_path);

    // Check if file exists
    if !std::path::Path::new(video_path).exists() {
        eprintln!("⚠️  Video file not found: {}", video_path);
        // Extra debug: show pointer to Path, and list contents of `videos/` folder if present
        let p = std::path::Path::new(video_path);
        println!("Debug: Path::new ptr = {:p}", p);
        match std::fs::read_dir("videos") {
            Ok(entries) => {
                println!("Debug: listing 'videos/' directory contents:");
                for entry in entries.flatten() {
                    println!("  - {:?}", entry.file_name());
                }
            }
            Err(e) => println!("Debug: cannot read 'videos/' dir: {}", e),
        }

        println!("   Server will run but no video stream available");
        println!("✅ Ready to accept RTSP connections");
        println!("   URL: rtsp://127.0.0.1:8554/cam");

        // Keep task alive
        loop {
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    }

    println!("📁 Video source: {}", video_path);

    // Create video source
    let source = FileSource::new(video_path.to_string());

    // Start FFmpeg process
    let mut child = source.start_ffmpeg()?;
    println!("Debug: FileSource addr = {:p}", &source);

    // Đọc stderr trong background thread để không block
    if let Some(mut stderr) = child.stderr.take() {
        tokio::spawn(async move {
            let mut buf = String::new();
            if stderr.read_to_string(&mut buf).is_ok() && !buf.is_empty() {
                println!("Debug: FFmpeg stderr output:\n{}", buf);
            }
        });
    }

    let stdout = child.stdout.take().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::Other, "Failed to capture FFmpeg stdout")
    })?;
    println!("Debug: Child process addr = {:p}", &child);
    println!("Debug: FFmpeg stdout (ChildStdout) addr = {:p}", &stdout as *const _);

    println!("✅ FFmpeg started");
    println!("✅ Ready to accept RTSP connections");
    println!("   URL: rtsp://127.0.0.1:8554/cam");
    println!("   Test: ffplay rtsp://127.0.0.1:8554/cam");
    println!("   Test: vlc rtsp://127.0.0.1:8554/cam");
    println!("\n🎬 Streaming...");

    // Setup UDP sockets cho RTP/RTCP
    let rtp_socket = Arc::new(UdpSocket::bind("0.0.0.0:6000").await?);
    println!("RTP socket address: {:p}", Arc::as_ptr(&rtp_socket));
    let rtcp_socket = Arc::new(UdpSocket::bind("0.0.0.0:6001").await?);
    println!("RTCP socket address: {:p}", Arc::as_ptr(&rtcp_socket));

    println!("📡 RTP socket: 0.0.0.0:6000");
    println!("📡 RTCP socket: 0.0.0.0:6001");

    // RTP Packetizer
    let packetizer = Arc::new(Mutex::new(H264Packetizer::new(0x12345678)));
    println!("Packetizer address: {:p}", Arc::as_ptr(&packetizer));

    // RTCP Sender Report
    let sender_report = Arc::new(Mutex::new(SenderReport::new(0x12345678)));
    println!("Sender report address: {:p}", Arc::as_ptr(&sender_report));

    // Spawn RTCP sender (gửi SR mỗi 5 giây)
    let rtcp_socket_clone = rtcp_socket.clone();
    let sender_report_clone = sender_report.clone();
    let state_clone = state.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(5)).await;

            let sr = sender_report_clone.lock().await;
            let sr_packet = sr.to_bytes();

            // Gửi đến tất cả UDP playing clients
            let udp_clients = state_clone.read().await.get_udp_clients();
            for (_rtp_addr, rtcp_addr) in udp_clients {
                if let Err(e) = rtcp_socket_clone.send_to(&sr_packet, rtcp_addr).await {
                    eprintln!("⚠️  RTCP send error to {}: {}", rtcp_addr, e);
                } else {
                    println!("📊 RTCP SR sent to {} - packets: {}, bytes: {}",
                             rtcp_addr, sr.packet_count, sr.octet_count);
                }
            }
        }
    });

    // Parse NALUs và gửi qua RTP
    let mut parser = NaluParser::new();
    let mut reader = std::io::BufReader::new(stdout);
    let mut buffer = [0u8; 8192];

    let mut frame_count = 0u64;

    // Cache SPS/PPS để gửi cho new clients
    let mut sps: Option<Vec<u8>> = None;
    let mut pps: Option<Vec<u8>> = None;
    let mut last_udp_clients_count = 0;

    loop {
        // Đọc data từ FFmpeg
        match reader.read(&mut buffer) {
            Ok(0) => {
                println!("📹 FFmpeg stream ended (loop will restart)");
                break;
            }
            Ok(n) => {
                // Parse NALUs
                let nalus = parser.parse(&buffer[..n]);

                if nalus.is_empty() {
                    continue;
                }

                // Get UDP playing clients (TCP clients are handled by their own sessions)
                let udp_clients = state.read().await.get_udp_clients();

                if udp_clients.is_empty() {
                    // No UDP clients playing, just consume the data
                    continue;
                }

                // Detect new clients and send SPS/PPS
                if udp_clients.len() > last_udp_clients_count {
                    if let (Some(ref sps_data), Some(ref pps_data)) = (&sps, &pps) {
                        println!("📡 New UDP client detected, sending SPS/PPS");

                        // Send SPS
                        let mut pac = packetizer.lock().await;
                        let sps_packets = pac.packetize(sps_data, false);
                        for packet in sps_packets {
                            let data = packet.to_bytes();
                            for (rtp_addr, _) in &udp_clients {
                                let _ = rtp_socket.send_to(&data, rtp_addr).await;
                            }
                        }

                        // Send PPS
                        let pps_packets = pac.packetize(pps_data, false);
                        for packet in pps_packets {
                            let data = packet.to_bytes();
                            for (rtp_addr, _) in &udp_clients {
                                let _ = rtp_socket.send_to(&data, rtp_addr).await;
                            }
                        }
                    }
                }
                last_udp_clients_count = udp_clients.len();

                // Process NALUs
                for (i, nalu) in nalus.iter().enumerate() {
                    if nalu.is_empty() {
                        continue;
                    }

                    let nalu_type = nalu[0] & 0x1F;

                    // Cache SPS (type 7) and PPS (type 8)
                    match nalu_type {
                        7 => {
                            sps = Some(nalu.clone());
                            println!("📦 Cached SPS (size: {} bytes)", nalu.len());
                        }
                        8 => {
                            pps = Some(nalu.clone());
                            println!("📦 Cached PPS (size: {} bytes)", nalu.len());
                        }
                        _ => {}
                    }

                    let is_keyframe = nalu_type == 5 || nalu_type == 7 || nalu_type == 8;

                    // Packetize NALU
                    let is_last = i == nalus.len() - 1;
                    let mut pac = packetizer.lock().await;
                    let packets = pac.packetize(nalu, is_last);

                    // Gửi các RTP packets đến tất cả UDP playing clients
                    for packet in packets {
                        let data = packet.to_bytes();
                        let size = data.len();

                        for (rtp_addr, _rtcp_addr) in &udp_clients {
                            if let Err(e) = rtp_socket.send_to(&data, rtp_addr).await {
                                eprintln!("⚠️  RTP send error to {}: {}", rtp_addr, e);
                            }
                        }

                        // Update RTCP statistics
                        let mut sr = sender_report.lock().await;
                        sr.add_packet(size);
                    }

                    if is_keyframe {
                        frame_count += 1;
                        if frame_count % 30 == 0 {
                            println!("🎬 Sent {} frames to {} UDP client(s) (NALU type: {})",
                                     frame_count, udp_clients.len(), nalu_type);
                        }
                    }
                }

                // Increment timestamp ONCE per frame group
                // Since we're using -re flag, FFmpeg outputs in real-time
                // We use 3000 ticks (90000Hz / 30fps) as default, but this will be
                // naturally paced by FFmpeg's -re flag
                let mut pac = packetizer.lock().await;
                pac.increment_timestamp(3000);
            }
            Err(e) => {
                eprintln!("❌ Read error: {}", e);
                break;
            }
        }
    }

    // Cleanup
    let _ = child.kill();

    Ok(())
}

