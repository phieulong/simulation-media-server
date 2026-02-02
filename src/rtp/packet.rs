use bytes::{BytesMut, BufMut};

/// RTP Header (12 bytes chuẩn)
#[derive(Debug, Clone)]
pub struct RtpHeader {
    pub version: u8,         // 2 bits, luôn = 2
    pub padding: bool,       // 1 bit
    pub extension: bool,     // 1 bit
    pub csrc_count: u8,      // 4 bits
    pub marker: bool,        // 1 bit - đánh dấu cuối frame
    pub payload_type: u8,    // 7 bits - 96 cho H.264
    pub sequence: u16,       // 16 bits - tăng dần
    pub timestamp: u32,      // 32 bits - 90kHz clock
    pub ssrc: u32,           // 32 bits - source identifier
}

impl RtpHeader {
    pub fn new(payload_type: u8, sequence: u16, timestamp: u32, ssrc: u32) -> Self {
        Self {
            version: 2,
            padding: false,
            extension: false,
            csrc_count: 0,
            marker: false,
            payload_type,
            sequence,
            timestamp,
            ssrc,
        }
    }

    /// Serialize header thành bytes
    pub fn to_bytes(&self) -> [u8; 12] {
        let mut header = [0u8; 12];
        
        // Byte 0: V(2) P(1) X(1) CC(4)
        header[0] = (self.version << 6) 
                  | ((self.padding as u8) << 5)
                  | ((self.extension as u8) << 4)
                  | (self.csrc_count & 0x0F);
        
        // Byte 1: M(1) PT(7)
        header[1] = ((self.marker as u8) << 7) | (self.payload_type & 0x7F);
        
        // Bytes 2-3: Sequence number
        header[2..4].copy_from_slice(&self.sequence.to_be_bytes());
        
        // Bytes 4-7: Timestamp
        header[4..8].copy_from_slice(&self.timestamp.to_be_bytes());
        
        // Bytes 8-11: SSRC
        header[8..12].copy_from_slice(&self.ssrc.to_be_bytes());
        
        header
    }
}

/// RTP Packet = Header + Payload
#[derive(Debug)]
pub struct RtpPacket {
    pub header: RtpHeader,
    pub payload: Vec<u8>,
}

impl RtpPacket {
    pub fn new(header: RtpHeader, payload: Vec<u8>) -> Self {
        Self { header, payload }
    }

    /// Serialize toàn bộ packet
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(12 + self.payload.len());
        buf.extend_from_slice(&self.header.to_bytes());
        buf.extend_from_slice(&self.payload);
        buf
    }
}
