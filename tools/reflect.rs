use pnet::datalink::Channel::Ethernet;
use pnet::datalink::{self, Config};

fn main() {
    let ifname = std::env::args().nth(1);
    let ifname = if let Some(ifname) = ifname {
        ifname
    } else {
        eprintln!(
            "Usage: {} IFNAME",
            std::env::args().next().unwrap_or("reflect".to_string())
        );
        std::process::exit(2)
    };

    let interface = datalink::interfaces()
        .into_iter()
        .find(|iface| iface.name == ifname)
        .expect("Network interface not found");

    let config = Config {
        // write_buffer_size: 64 * 1024 * 1024,
        read_buffer_size: 64 * 1024 * 1024,
        ..Default::default()
    };

    let (mut tx, mut rx) = match datalink::channel(&interface, config) {
        Ok(Ethernet(tx, rx)) => (tx, rx),
        Ok(_) => panic!("Unhandled channel type"),
        Err(e) => panic!(
            "An error occurred when creating the datalink channel: {}",
            e
        ),
    };

    loop {
        match rx.next() {
            Ok(packet_raw) => {
                tx.send_to(packet_raw, None);
            }
            Err(e) => {
                panic!("An error occurred while reading: {}", e);
            }
        }
    }
}
