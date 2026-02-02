use std::time::{SystemTime, UNIX_EPOCH};

/// RTCP Sender Report (SR)
/// Gửi thống kê về stream để client không timeout
#[derive(Debug)]
pub struct SenderReport {
    pub ssrc: u32,
    pub packet_count: u32,
    pub octet_count: u32,
}

impl SenderReport {
    pub fn new(ssrc: u32) -> Self {
        Self {
            ssrc,
            packet_count: 0,
            octet_count: 0,
        }
    }

    /// Update counters
    pub fn add_packet(&mut self, size: usize) {
        self.packet_count += 1;
        self.octet_count += size as u32;
    }

    /// Serialize SR packet theo RFC 3550
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(28);
        
        // RTCP Header
        // V=2, P=0, RC=0, PT=200 (SR)
        buf.push(0x80); // V=2, P=0, RC=0
        buf.push(200);  // PT=200 (Sender Report)
        
        // Length in 32-bit words - 1 (6 words = 24 bytes payload)
        buf.extend_from_slice(&6u16.to_be_bytes());
        
        // SSRC of sender
        buf.extend_from_slice(&self.ssrc.to_be_bytes());
        
        // NTP Timestamp (64 bits)
        let (ntp_secs, ntp_frac) = Self::get_ntp_timestamp();
        buf.extend_from_slice(&ntp_secs.to_be_bytes());
        buf.extend_from_slice(&ntp_frac.to_be_bytes());
        
        // RTP Timestamp (32 bits) - tương ứng với NTP
        let rtp_ts = Self::ntp_to_rtp_timestamp(ntp_secs, ntp_frac);
        buf.extend_from_slice(&rtp_ts.to_be_bytes());
        
        // Sender's packet count
        buf.extend_from_slice(&self.packet_count.to_be_bytes());
        
        // Sender's octet count
        buf.extend_from_slice(&self.octet_count.to_be_bytes());
        
        buf
    }

    /// Get NTP timestamp (seconds, fractional seconds)
    fn get_ntp_timestamp() -> (u32, u32) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap();
        
        // NTP epoch is 1900, Unix epoch is 1970 (70 years difference)
        const NTP_OFFSET: u64 = 2_208_988_800;
        
        let secs = now.as_secs() + NTP_OFFSET;
        let nanos = now.subsec_nanos();
        
        // Fractional part: nanoseconds to 32-bit fraction
        let frac = ((nanos as u64) << 32) / 1_000_000_000;
        
        (secs as u32, frac as u32)
    }

    /// Convert NTP to RTP timestamp (90kHz)
    fn ntp_to_rtp_timestamp(ntp_secs: u32, ntp_frac: u32) -> u32 {
        // Simplified: chỉ lấy giây * 90000
        // Trong production nên chính xác hơn
        let secs = ntp_secs as u64;
        let frac = (ntp_frac as u64) * 1_000_000_000 >> 32;
        let total_nanos = secs * 1_000_000_000 + frac;
        
        ((total_nanos * 90) / 1_000_000) as u32
    }
}
