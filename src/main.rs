mod source;
mod rtsp;
mod rtp;
mod rtcp;

use rtsp::server::RtspServer;
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
    
    // Start RTSP server
    let rtsp_server = RtspServer::new("0.0.0.0:8554".to_string());
    
    let rtsp_handle = tokio::spawn(async move {
        if let Err(e) = rtsp_server.run().await {
            eprintln!("❌ RTSP Server error: {}", e);
        }
    });

    // Start RTP/RTCP streaming task (simplified for demo)
    let streaming_handle = tokio::spawn(async move {
        // Đợi một chút để RTSP server khởi động
        tokio::time::sleep(Duration::from_secs(2)).await;
        
        println!("\n📹 Starting video streaming...");
        println!("=====================================");
        
        // Kiểm tra xem có file test không
        // Nếu không có, sẽ chỉ chạy RTSP server thôi
        
        // TODO: Implement RTP streaming khi có client PLAY
        // Hiện tại chỉ chạy RTSP server
        
        println!("✅ Ready to accept RTSP connections");
        println!("   URL: rtsp://127.0.0.1:8554/cam");
        println!("   Test with: ffplay rtsp://127.0.0.1:8554/cam");
        println!("   or VLC: vlc rtsp://127.0.0.1:8554/cam");
    });

    // Wait for both tasks
    let _ = tokio::join!(rtsp_handle, streaming_handle);
}
