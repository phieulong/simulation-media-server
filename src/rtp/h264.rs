use super::packet::{RtpHeader, RtpPacket};

const MTU: usize = 1400; // Max RTP payload size (để tránh fragmentation)

/// H.264 RTP Packetizer theo RFC 6184
pub struct H264Packetizer {
    sequence: u16,
    timestamp: u32,
    ssrc: u32,
    payload_type: u8,
}

impl H264Packetizer {
    pub fn new(ssrc: u32) -> Self {
        Self {
            sequence: 0,
            timestamp: 0,
            ssrc,
            payload_type: 96, // Dynamic payload type cho H.264
        }
    }

    /// Packetize một NALU thành 1 hoặc nhiều RTP packets
    pub fn packetize(&mut self, nalu: &[u8], is_last: bool) -> Vec<RtpPacket> {
        if nalu.is_empty() {
            return Vec::new();
        }

        let mut packets = Vec::new();

        // NALU nhỏ: gửi trọn trong 1 RTP packet (Single NAL Unit mode)
        if nalu.len() <= MTU {
            let mut header = RtpHeader::new(
                self.payload_type,
                self.sequence,
                self.timestamp,
                self.ssrc,
            );
            header.marker = is_last; // Đánh dấu cuối frame
            
            let packet = RtpPacket::new(header, nalu.to_vec());
            packets.push(packet);
            
            self.sequence = self.sequence.wrapping_add(1);
        } else {
            // NALU lớn: chia nhỏ bằng FU-A (Fragmentation Unit)
            packets = self.fragment_nalu(nalu, is_last);
        }

        packets
    }

    /// Fragment NALU lớn thành nhiều FU-A packets
    fn fragment_nalu(&mut self, nalu: &[u8], is_last_nalu: bool) -> Vec<RtpPacket> {
        let mut packets = Vec::new();
        
        let nalu_header = nalu[0];
        let nalu_payload = &nalu[1..];
        
        // FU Indicator: giống NALU header nhưng type = 28 (FU-A)
        let fu_indicator = (nalu_header & 0xE0) | 28;
        
        // Chia payload thành chunks
        let chunks: Vec<&[u8]> = nalu_payload
            .chunks(MTU - 2) // -2 cho FU indicator + FU header
            .collect();
        
        for (i, chunk) in chunks.iter().enumerate() {
            let is_first = i == 0;
            let is_last = i == chunks.len() - 1;
            
            // FU Header: S(1) E(1) R(1) Type(5)
            let mut fu_header = nalu_header & 0x1F; // Lấy NAL type
            if is_first {
                fu_header |= 0x80; // Set Start bit
            }
            if is_last {
                fu_header |= 0x40; // Set End bit
            }
            
            // Payload = FU indicator + FU header + data
            let mut payload = Vec::with_capacity(2 + chunk.len());
            payload.push(fu_indicator);
            payload.push(fu_header);
            payload.extend_from_slice(chunk);
            
            let mut header = RtpHeader::new(
                self.payload_type,
                self.sequence,
                self.timestamp,
                self.ssrc,
            );
            
            // Marker bit chỉ set ở packet cuối cùng của frame cuối
            header.marker = is_last && is_last_nalu;
            
            packets.push(RtpPacket::new(header, payload));
            self.sequence = self.sequence.wrapping_add(1);
        }
        
        packets
    }

    /// Tăng timestamp (gọi sau mỗi frame)
    /// Với 30fps: timestamp += 90000/30 = 3000
    pub fn increment_timestamp(&mut self, duration_90khz: u32) {
        self.timestamp = self.timestamp.wrapping_add(duration_90khz);
    }

    pub fn set_timestamp(&mut self, ts: u32) {
        self.timestamp = ts;
    }
}
