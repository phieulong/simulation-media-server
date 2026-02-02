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
                "-profile:v", "baseline",       // Baseline profile cho compatibility
                "-level", "3.1",                // H.264 level 3.1
                "-pix_fmt", "yuv420p",          // Pixel format
                "-g", "30",                     // GOP size (keyframe every 30 frames)
                "-keyint_min", "30",            // Minimum keyframe interval
                "-bf", "0",                     // No B-frames cho low latency
                "-x264-params", "nal-hrd=cbr:force-cfr=1", // Constant bitrate for stable streaming
                "-f", "h264",                   // Format H.264 raw
                "-bsf:v", "h264_mp4toannexb",  // Ensure Annex-B format
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
        
        let mut i = 0;
        while i < self.buffer.len() {
            // Find start code at position i
            if let Some((sc_start, sc_len)) = self.find_start_code_at(i) {
                // Found a start code, now find the next one
                let nalu_start = sc_start + sc_len;

                if let Some((next_sc_start, _)) = self.find_start_code_at(nalu_start) {
                    // Found next start code, extract NALU between them
                    let nalu = self.buffer[nalu_start..next_sc_start].to_vec();
                    if !nalu.is_empty() && nalu.len() > 0 {
                        nalus.push(nalu);
                    }
                    i = next_sc_start;
                } else {
                    // No more start codes, keep remaining in buffer
                    self.buffer = self.buffer[sc_start..].to_vec();
                    break;
                }
            } else {
                // No start code found, keep everything in buffer
                break;
            }
        }

        // If we processed all, clear buffer
        if i > 0 && i >= self.buffer.len() {
            self.buffer.clear();
        }
        
        nalus
    }

    fn find_start_code_at(&self, start: usize) -> Option<(usize, usize)> {
        if start >= self.buffer.len() {
            return None;
        }

        for i in start..self.buffer.len().saturating_sub(3) {
            // 4-byte start code: 0x00 0x00 0x00 0x01
            if i + 3 < self.buffer.len()
                && self.buffer[i] == 0
                && self.buffer[i+1] == 0
                && self.buffer[i+2] == 0
                && self.buffer[i+3] == 1 {
                return Some((i, 4));
            }
            // 3-byte start code: 0x00 0x00 0x01
            if i + 2 < self.buffer.len()
                && self.buffer[i] == 0
                && self.buffer[i+1] == 0
                && self.buffer[i+2] == 1 {
                return Some((i, 3));
            }
        }
        None
    }
}
