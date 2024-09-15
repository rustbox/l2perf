use std::{
    io::{stderr, Write},
    str::FromStr,
};

use pnet::{datalink::Channel::Ethernet, packet::ethernet::EtherType, util::MacAddr};
use pnet::{
    datalink::{self, Config},
    packet::ethernet::MutableEthernetPacket,
};
use rand::{seq::SliceRandom, Rng};

fn usage<Writer: Write>(w: &mut Writer) {
    writeln!(
        w,
        r#"Usage: {0} IFNAME CTRL [DATA]

A virtual GPY111 test packet generator. The CTRL and DATA arguments accept
16-bit values and correspond to the PHY_TPGCTRL and PHY_TPGDATA registers
respectively.

NB: CTRL should always be a multiple of 3 to both enable & activate the TPG,
    and must also have bit 7 cleared (0).

Example:
    {0} veth1 0x22_73 0b1001_1111_01010101
"#,
        std::env::args().next().unwrap_or_else(|| "tpg".to_string())
    )
    .unwrap();
}

#[derive(Default, Debug, Clone, Copy)]
pub struct Reg(pub u16);

impl FromStr for Reg {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let sv = s.to_string();
        let sv = sv.replace("_", "");
        let sv = sv.replace("'", "");
        Ok(Reg(match sv.as_str() {
            sv if sv.starts_with("0x") => u16::from_str_radix(&sv[2..], 16)
                .map_err(|err| format!("couldn't read {:?} as hex: {}", s, err))?,

            sv if sv.starts_with("0o") => u16::from_str_radix(&sv[2..], 8)
                .map_err(|err| format!("couldn't read {:?} as octal: {}", s, err))?,

            sv if sv.starts_with("0b") => u16::from_str_radix(&sv[2..], 2)
                .map_err(|err| format!("couldn't read {:?} as binary: {}", s, err))?,

            #[allow(clippy::from_str_radix_10)]
            _ => u16::from_str_radix(&sv, 10)
                .map_err(|err| format!("couldn't read {:?}: {}", s, err))?,
        }))
    }
}

impl Reg {
    // idx should be in [0, 3]
    pub fn nibble(&self, idx: u8) -> u8 {
        ((self.0 >> (idx * 4)) & 0xf) as u8
    }

    // idx should be in [0, 1]
    pub fn byte(&self, idx: u8) -> u8 {
        (self.0 >> (idx * 8)) as u8
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Ctrl {
    pub bits: Reg,
}

impl FromStr for Ctrl {
    type Err = <Reg as FromStr>::Err;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bits: Reg = s.parse()?;
        if (bits.0 >> 7) & 0b1 != 0 {
            return Err(format!(
                "invalid value: reserved bit 7 is set (try: 0x{:x?} instead?)",
                bits.0 ^ (0b1 << 7)
            ));
        }

        Ok(Ctrl { bits })
    }
}

impl Ctrl {
    // TODO "Depending on the MODE, the TPG sends only 1 single packet or chunks of 10,000 packets until stopped"
    pub fn start(&self) -> bool {
        self.bits.0 & (1 << 1) != 0
    }
    pub fn enable(&self) -> bool {
        self.bits.0 & (1 << 0) != 0
    }

    pub fn should_run(&self) -> bool {
        self.enable() && self.start()
    }

    pub fn mode(&self) -> Mode {
        if (self.bits.0 >> 13) & 0x1 == 0 {
            Mode::Continuous
        } else {
            Mode::Single
        }
    }

    pub fn size(&self) -> SizeOpt {
        match (self.bits.0 >> 4) & 0x7 {
            0b000 => SizeOpt::Fixed { len: 64 },
            0b001 => SizeOpt::Fixed { len: 2048 }, // jumbo
            0b010 => SizeOpt::Fixed { len: 256 },
            0b011 => SizeOpt::Fixed { len: 4096 }, // jumbo
            0b100 => SizeOpt::Fixed { len: 1024 },
            0b101 => SizeOpt::Fixed { len: 1518 },
            0b110 => SizeOpt::Fixed { len: 9000 }, // jumbo

            0b111 => SizeOpt::Random,

            8..=u16::MAX => unreachable!(),
        }
    }

    pub fn ipgl(&self) -> InterPacketGap {
        match (self.bits.0 >> 10) & 0b11 {
            0b00 => InterPacketGap { bitlen: 48 },
            0b01 => InterPacketGap { bitlen: 96 },
            0b10 => InterPacketGap { bitlen: 960 },
            0b11 => InterPacketGap { bitlen: 9600 },

            4..=u16::MAX => unreachable!(),
        }
    }

    pub fn ptype(&self) -> PacketType {
        match (self.bits.0 >> 8) & 0b11 {
            0b00 => PacketType::Random,
            0b01 => PacketType::ByteInc,
            0b10 => PacketType::Predefined,
            0b11 => todo!("Debug packet type not yet implemented"),

            4..=u16::MAX => unreachable!(),
        }
    }

    // TODO "debug dump"

    pub fn chsel(&self) -> ! {
        todo!()
    }
    pub fn mopt(&self) -> ! {
        todo!()
    }
}

#[derive(Clone, Copy)]
pub enum Mode {
    Continuous,
    // NB: "single" can also mean four sometimes in "debug dumping mode"
    // TODO: "debug dumping mode"
    Single,
}

#[derive(Clone, Copy)]
pub struct InterPacketGap {
    pub bitlen: u16,
}

#[derive(Clone, Copy)]
pub enum SizeOpt {
    // len is total frame size (including 14-byte ethernet header & 4-byte trailer)
    Fixed { len: u16 },
    // choose from {64, 256, 1024, 1518}
    Random,
}

impl SizeOpt {
    pub fn is_jumbo(&self) -> bool {
        match self {
            SizeOpt::Fixed { len } => *len > 1518,
            // random sizes will never be jumbo
            SizeOpt::Random => false,
        }
    }
}

pub enum PacketType {
    Random,
    ByteInc,
    Predefined,
    // TODO:
    // Debug,
}

#[derive(Default, Debug, Clone, Copy)]
pub struct Data {
    bits: Reg,
}

impl FromStr for Data {
    type Err = <Reg as FromStr>::Err;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Data { bits: s.parse()? })
    }
}

impl Data {
    fn dest_addr(&self) -> MacAddr {
        MacAddr(0x00, 0x03, 0x19, 0xff, 0xff, 0xf0 | self.bits.nibble(3))
    }

    fn src_addr(&self) -> MacAddr {
        MacAddr(0x00, 0x03, 0x19, 0xff, 0xff, 0xf0 | self.bits.nibble(2))
    }

    fn frame_data(&self) -> u8 {
        self.bits.byte(0)
    }
}

fn main() {
    let ifname = std::env::args().nth(1);
    let ifname = if let Some(ifname) = ifname {
        ifname
    } else {
        usage(&mut stderr());
        std::process::exit(2)
    };

    let ctrl: Ctrl = match std::env::args()
        .nth(2)
        .ok_or("missing required argument: CTRL".to_string())
        .and_then(|s| s.parse())
    {
        Ok(ctrl) => ctrl,
        Err(err) => {
            usage(&mut stderr());
            eprintln!("{}", err);
            std::process::exit(2)
        }
    };

    let data: Data = match std::env::args().nth(3).map(|s| s.parse()) {
        None => Data::default(),
        Some(Ok(data)) => data,
        Some(Err(err)) => {
            usage(&mut stderr());
            eprintln!("{}", err);
            std::process::exit(2)
        }
    };

    if !ctrl.should_run() {
        eprintln!(
            "ctrl register should be both enabled and started, saw: 0x{:x?}",
            ctrl.bits.0
        );
        std::process::exit(1)
    }

    let interface = datalink::interfaces()
        .into_iter()
        .find(|iface| iface.name == ifname)
        .expect("Network interface not found");

    let config = Config {
        // write_buffer_size: 64 * 1024 * 1024,
        read_buffer_size: 64 * 1024 * 1024,
        ..Default::default()
    };

    match ctrl.mode() {
        Mode::Single => {}
        Mode::Continuous => todo!("continuous mode"),
    };

    let (mut tx, _) = match datalink::channel(&interface, config) {
        Ok(Ethernet(tx, rx)) => (tx, rx),
        Ok(_) => panic!("Unhandled channel type"),
        Err(e) => panic!(
            "An error occurred when creating the datalink channel: {}",
            e
        ),
    };

    let mut rng = rand::thread_rng();

    let payload_size = |sz: u16| -> usize {
        // per p.75 of the datasheet, this is "{64,128,256,512,1024,1518,9600}-14 octets"
        // but:
        // - there is no way to specify 128 or 512 as the SIZE field?
        // - the brackets indicating size stretch from the header start to FCS end,
        //   yet the header on its own is 14 octets; also, 1518-14=1504 which is very strange
        // - the datasheet claims that the FCS is "2 octets", it ought to be 4?

        // TODO confirm with experimental result
        sz as usize - 18
    };
    let size = match ctrl.size() {
        SizeOpt::Fixed { len } => payload_size(len),
        SizeOpt::Random => payload_size(*[64_u16, 256, 1024, 1518].choose(&mut rng).unwrap()),
    };

    let mut buf = vec![0u8; size + 14 /* headers */];
    let mut packet_gen = |buf: &mut [u8]| {
        let mut packet = MutableEthernetPacket::new(buf).unwrap();
        // packet.set_ethertype(EtherType::new(opts.ethertype));
        packet.set_ethertype(EtherType::new(size as u16));
        packet.set_source(data.src_addr());
        packet.set_destination(data.dest_addr());

        for i in 0..size {
            buf[14 + i] = match ctrl.ptype() {
                // TODO these are all guesses too
                PacketType::Random => rng.gen(),
                PacketType::ByteInc => i as u8,
                PacketType::Predefined => data.frame_data(),
            }
        }
        // packet.set_payload(vals)
    };

    packet_gen(&mut buf[..]);

    // TODO continuous mode?
    // tx.build_and_send(10_000, packet_size, func)

    // loop {
    //     match rx.next() {
    //         Ok(packet_raw) => {
    //             tx.send_to(packet_raw, None);
    //         }
    //         Err(e) => {
    //             panic!("An error occurred while reading: {}", e);
    //         }
    //     }
    // }

    tx.send_to(&buf[..], None).unwrap().unwrap();
}
