use std::process::{Command, Stdio};
use std::io::{Read, BufReader};

/// Video source từ file MP4, loop vô hạn
pub struct FileSource {
    pub file_path: String,
}

impl FileSource {
    pub fn new(file_path: String) -> Self {
        Self { file_path }
    }

    /// Tạo FFmpeg process để encode file MP4 thành H.264 raw stream
    /// Output: H.264 NALUs qua stdout
    pub fn start_ffmpeg(&self) -> std::io::Result<std::process::Child> {
        // Debug: print ffmpeg command
        println!("Debug: FFmpeg command:");
        println!("  ffmpeg -re -stream_loop -1 -i {:?} -an -c:v libx264 -preset ultrafast -tune zerolatency -f h264 pipe:1", &self.file_path);
        
        // Check if ffmpeg exists
        let ffmpeg_check = Command::new("which")
            .arg("ffmpeg")
            .output();
        
        match ffmpeg_check {
            Ok(output) => {
                if output.status.success() {
                    let path = String::from_utf8_lossy(&output.stdout);
                    println!("Debug: FFmpeg found at: {}", path.trim());
                } else {
                    println!("Debug: FFmpeg NOT found in PATH");
                }
            }
            Err(e) => println!("Debug: Error checking ffmpeg: {}", e),
        }
        
        // Check if input file exists
        println!("Debug: Input file path: {:?}", &self.file_path);
        println!("Debug: File exists: {}", std::path::Path::new(&self.file_path).exists());
        
        let child = Command::new("ffmpeg")
            .args(&[
                "-re",                          // Real-time mode
                "-stream_loop", "-1",           // Loop vô hạn
                "-i", &self.file_path,          // Input file
                "-an",                          // Không có audio
                "-c:v", "libx264",              // H.264 codec
                "-preset", "ultrafast",         // Encode nhanh
                "-tune", "zerolatency",         // Low latency
                "-f", "h264",                   // Format H.264 raw
                "pipe:1"                        // Output to stdout
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())             // Capture stderr để xem lỗi
            .spawn()?;

        Ok(child)
    }
}

/// Parser để tách NALUs từ H.264 stream
pub struct NaluParser {
    buffer: Vec<u8>,
}

impl NaluParser {
    pub fn new() -> Self {
        Self {
            buffer: Vec::new(),
        }
    }

    /// Parse NALUs từ buffer
    /// Return: Vec của các NALU (không bao gồm start code)
    pub fn parse(&mut self, data: &[u8]) -> Vec<Vec<u8>> {
        self.buffer.extend_from_slice(data);
        let mut nalus = Vec::new();
        
        // Tìm start code: 0x00 0x00 0x00 0x01 hoặc 0x00 0x00 0x01
        let mut pos = 0;
        while pos < self.buffer.len() {
            if let Some(start) = self.find_start_code(pos) {
                if pos > 0 && pos != start {
                    // Có NALU từ vị trí trước đến start code này
                    let nalu = self.buffer[pos..start].to_vec();
                    if !nalu.is_empty() {
                        nalus.push(nalu);
                    }
                }
                
                // Skip start code (3 hoặc 4 bytes)
                pos = start;
                if pos + 3 < self.buffer.len() 
                    && self.buffer[pos..pos+4] == [0, 0, 0, 1] {
                    pos += 4;
                } else if pos + 2 < self.buffer.len() 
                    && self.buffer[pos..pos+3] == [0, 0, 1] {
                    pos += 3;
                } else {
                    pos += 1;
                }
            } else {
                break;
            }
        }
        
        // Giữ lại phần chưa parse được
        if pos < self.buffer.len() {
            self.buffer = self.buffer[pos..].to_vec();
        } else {
            self.buffer.clear();
        }
        
        nalus
    }

    fn find_start_code(&self, start: usize) -> Option<usize> {
        for i in start..self.buffer.len().saturating_sub(3) {
            // 4-byte start code
            if self.buffer[i] == 0 && self.buffer[i+1] == 0 
                && self.buffer[i+2] == 0 && self.buffer[i+3] == 1 {
                return Some(i);
            }
            // 3-byte start code
            if self.buffer[i] == 0 && self.buffer[i+1] == 0 
                && self.buffer[i+2] == 1 {
                return Some(i);
            }
        }
        None
    }
}
