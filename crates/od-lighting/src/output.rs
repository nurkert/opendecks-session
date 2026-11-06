//! DMX outputs. Each turns a 512-channel universe into the bytes of one
//! protocol: Art-Net (ArtDmx over UDP 6454), sACN / E1.31 (UDP 5568) and the
//! Enttec DMX USB Pro serial protocol (label 6). Packet builders are pure so
//! they can be checked byte for byte; senders wrap them.

use std::io::Write;
use std::net::{SocketAddr, UdpSocket};

/// One DMX universe: 512 channel values (channel 1 = index 0).
pub type Universe = [u8; 512];

/// ArtDmx packet (Art-Net 4) for `universe` (15-bit port address).
pub fn artnet_packet(universe: u16, sequence: u8, data: &Universe) -> Vec<u8> {
    let mut p = Vec::with_capacity(18 + 512);
    p.extend_from_slice(b"Art-Net\0");
    p.extend_from_slice(&0x5000u16.to_le_bytes()); // OpDmx
    p.extend_from_slice(&14u16.to_be_bytes()); // protocol version
    p.push(sequence);
    p.push(0); // physical
    p.extend_from_slice(&(universe & 0x7FFF).to_le_bytes()); // SubUni, Net
    p.extend_from_slice(&512u16.to_be_bytes());
    p.extend_from_slice(data);
    p
}

/// sACN (ANSI E1.31-2018) data packet.
pub fn sacn_packet(
    universe: u16,
    sequence: u8,
    cid: &[u8; 16],
    source_name: &str,
    priority: u8,
    data: &Universe,
) -> Vec<u8> {
    let mut p = Vec::with_capacity(638);
    // Root layer.
    p.extend_from_slice(&0x0010u16.to_be_bytes()); // preamble size
    p.extend_from_slice(&0x0000u16.to_be_bytes()); // postamble size
    p.extend_from_slice(b"ASC-E1.17\0\0\0");
    p.extend_from_slice(&(0x7000u16 | (638 - 16)).to_be_bytes()); // flags + length
    p.extend_from_slice(&0x0000_0004u32.to_be_bytes()); // VECTOR_ROOT_E131_DATA
    p.extend_from_slice(cid);
    // Framing layer.
    p.extend_from_slice(&(0x7000u16 | (638 - 38)).to_be_bytes());
    p.extend_from_slice(&0x0000_0002u32.to_be_bytes()); // VECTOR_E131_DATA_PACKET
    let mut name = [0u8; 64];
    let bytes = source_name.as_bytes();
    name[..bytes.len().min(63)].copy_from_slice(&bytes[..bytes.len().min(63)]);
    p.extend_from_slice(&name);
    p.push(priority);
    p.extend_from_slice(&0u16.to_be_bytes()); // sync address
    p.push(sequence);
    p.push(0); // options
    p.extend_from_slice(&universe.to_be_bytes());
    // DMP layer.
    p.extend_from_slice(&(0x7000u16 | (638 - 115)).to_be_bytes());
    p.push(0x02); // VECTOR_DMP_SET_PROPERTY
    p.push(0xA1); // address type & data type
    p.extend_from_slice(&0u16.to_be_bytes()); // first property address
    p.extend_from_slice(&1u16.to_be_bytes()); // address increment
    p.extend_from_slice(&513u16.to_be_bytes()); // property count
    p.push(0); // DMX start code
    p.extend_from_slice(data);
    p
}

/// sACN multicast group of a universe (239.255.hi.lo).
pub fn sacn_multicast(universe: u16) -> std::net::Ipv4Addr {
    std::net::Ipv4Addr::new(239, 255, (universe >> 8) as u8, universe as u8)
}

/// Enttec DMX USB Pro "Output Only Send DMX Packet" (label 6) message.
pub fn enttec_pro_packet(data: &Universe) -> Vec<u8> {
    let len = (data.len() + 1) as u16;
    let mut p = Vec::with_capacity(5 + len as usize + 1);
    p.push(0x7E);
    p.push(6);
    p.extend_from_slice(&len.to_le_bytes());
    p.push(0); // start code
    p.extend_from_slice(data);
    p.push(0xE7);
    p
}

/// Where a universe goes.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum OutputKind {
    /// Art-Net to an address (unicast or the broadcast address).
    ArtNet { target: SocketAddr, universe: u16 },
    /// sACN multicast for the universe.
    Sacn { universe: u16 },
    /// Enttec DMX USB Pro (or compatible) serial device path.
    EnttecPro { device: String },
}

/// A running output.
pub struct Output {
    kind: OutputKind,
    socket: Option<UdpSocket>,
    serial: Option<std::fs::File>,
    sequence: u8,
    cid: [u8; 16],
}

impl Output {
    pub fn open(kind: OutputKind) -> std::io::Result<Self> {
        let (socket, serial) = match &kind {
            OutputKind::ArtNet { .. } => {
                let s = UdpSocket::bind("0.0.0.0:0")?;
                s.set_broadcast(true)?;
                (Some(s), None)
            }
            OutputKind::Sacn { .. } => (Some(UdpSocket::bind("0.0.0.0:0")?), None),
            OutputKind::EnttecPro { device } => (
                None,
                Some(std::fs::OpenOptions::new().write(true).open(device)?),
            ),
        };
        // A stable component id per process (sACN requires one).
        let mut cid = *b"OpenDecks-Light!";
        cid[15] = (std::process::id() & 0xFF) as u8;
        Ok(Self {
            kind,
            socket,
            serial,
            sequence: 0,
            cid,
        })
    }

    /// Sends one frame of `data`.
    pub fn send(&mut self, data: &Universe) -> std::io::Result<()> {
        self.sequence = self.sequence.wrapping_add(1).max(1);
        match &self.kind {
            OutputKind::ArtNet { target, universe } => {
                let p = artnet_packet(*universe, self.sequence, data);
                self.socket
                    .as_ref()
                    .expect("socket")
                    .send_to(&p, target)
                    .map(|_| ())
            }
            OutputKind::Sacn { universe } => {
                let p = sacn_packet(
                    *universe,
                    self.sequence,
                    &self.cid,
                    "OpenDecks Session",
                    100,
                    data,
                );
                let addr = SocketAddr::from((sacn_multicast(*universe), 5568));
                self.socket
                    .as_ref()
                    .expect("socket")
                    .send_to(&p, addr)
                    .map(|_| ())
            }
            OutputKind::EnttecPro { .. } => self
                .serial
                .as_mut()
                .expect("serial")
                .write_all(&enttec_pro_packet(data)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn universe() -> Universe {
        let mut u = [0u8; 512];
        u[0] = 255;
        u[511] = 7;
        u
    }

    #[test]
    fn artnet_layout() {
        let p = artnet_packet(0x0123, 9, &universe());
        assert_eq!(p.len(), 530);
        assert_eq!(&p[..8], b"Art-Net\0");
        assert_eq!(&p[8..10], &[0x00, 0x50]); // OpDmx, little endian
        assert_eq!(&p[10..12], &[0, 14]);
        assert_eq!(p[12], 9);
        assert_eq!(&p[14..16], &[0x23, 0x01]);
        assert_eq!(&p[16..18], &[2, 0]); // 512, big endian
        assert_eq!((p[18], p[529]), (255, 7));
    }

    #[test]
    fn sacn_layout() {
        let cid = [1u8; 16];
        let p = sacn_packet(1, 3, &cid, "OpenDecks", 100, &universe());
        assert_eq!(p.len(), 638);
        assert_eq!(&p[4..16], b"ASC-E1.17\0\0\0");
        assert_eq!(u16::from_be_bytes([p[16], p[17]]) & 0x0FFF, 622);
        assert_eq!(&p[22..38], &cid);
        assert_eq!(u16::from_be_bytes([p[38], p[39]]) & 0x0FFF, 600);
        assert_eq!(&p[44..53], b"OpenDecks");
        assert_eq!(p[108], 100); // priority
        assert_eq!(p[111], 3); // sequence
        assert_eq!(u16::from_be_bytes([p[113], p[114]]), 1); // universe
        assert_eq!(u16::from_be_bytes([p[115], p[116]]) & 0x0FFF, 523);
        assert_eq!(u16::from_be_bytes([p[123], p[124]]), 513);
        assert_eq!((p[125], p[126], p[637]), (0, 255, 7));
        assert_eq!(sacn_multicast(1), std::net::Ipv4Addr::new(239, 255, 0, 1));
    }

    #[test]
    fn enttec_layout() {
        let p = enttec_pro_packet(&universe());
        assert_eq!(&p[..4], &[0x7E, 6, 0x01, 0x02]); // 513 bytes
        assert_eq!((p[4], p[5], p[516], p[517]), (0, 255, 7, 0xE7));
    }

    #[test]
    fn artnet_reaches_a_listener() {
        let rx = UdpSocket::bind("127.0.0.1:0").unwrap();
        rx.set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        let mut out = Output::open(OutputKind::ArtNet {
            target: rx.local_addr().unwrap(),
            universe: 0,
        })
        .unwrap();
        out.send(&universe()).unwrap();
        let mut buf = [0u8; 600];
        let n = rx.recv(&mut buf).unwrap();
        assert_eq!((n, buf[18]), (530, 255));
    }
}
