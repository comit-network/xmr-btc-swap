use data_encoding::BASE32;
use libp2p::multiaddr::Protocol;
use libp2p::Multiaddr;

/// Tor expects an address format of ADDR:PORT.
/// This helper function tries to convert the provided multi-address into this
/// format. None is returned if an unsupported protocol was provided.
pub fn fmt_as_tor_compatible_address(multi: Multiaddr) -> Option<String> {
    let mut protocols = multi.iter();
    let address_string = match protocols.next()? {
        // if it is an Onion address, we have all we need and can return
        Protocol::Onion3(addr) => {
            return Some(format!(
                "{}.onion:{}",
                BASE32.encode(addr.hash()).to_lowercase(),
                addr.port()
            ));
        }
        // Deal with non-onion addresses
        Protocol::Ip4(addr) => format!("{}", addr),
        Protocol::Ip6(addr) => format!("{}", addr),
        Protocol::Dns(addr) => format!("{}", addr),
        Protocol::Dns4(addr) => format!("{}", addr),
        _ => return None,
    };

    let port = match protocols.next()? {
        Protocol::Tcp(port) => port,
        Protocol::Udp(port) => port,
        _ => return None,
    };

    Some(format!("{}:{}", address_string, port))
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_fmt_as_tor_compatible_address() {
        let test_cases = &[
            ("/onion3/oarchy4tamydxcitaki6bc2v4leza6v35iezmu2chg2bap63sv6f2did:1024/p2p/12D3KooWPD4uHN74SHotLN7VCH7Fm8zZgaNVymYcpeF1fpD2guc9", Some("oarchy4tamydxcitaki6bc2v4leza6v35iezmu2chg2bap63sv6f2did.onion:1024")),
                ("/ip4/127.0.0.1/tcp/7777", Some("127.0.0.1:7777")),
                ("/ip6/2001:db8:85a3:8d3:1319:8a2e:370:7348/tcp/7777", Some("2001:db8:85a3:8d3:1319:8a2e:370:7348:7777")),
                ("/ip4/127.0.0.1/udp/7777", Some("127.0.0.1:7777")),
                ("/ip4/127.0.0.1/tcp/7777/ws", Some("127.0.0.1:7777")),
                ("/dns4/randomdomain.com/tcp/7777", Some("randomdomain.com:7777")),
                ("/dns/randomdomain.com/tcp/7777", Some("randomdomain.com:7777")),
                ("/dnsaddr/randomdomain.com", None),
        ];

        for (multiaddress, expected_address) in test_cases {
            let actual_address =
                fmt_as_tor_compatible_address(multiaddress.parse().expect("a valid multi-address"));

            assert_eq!(&actual_address.as_deref(), expected_address)
        }
    }
}
