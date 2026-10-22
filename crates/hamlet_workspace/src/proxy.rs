//! Public HTTP egress through an operation-mounted Unix socket.

use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UnixListener, UnixStream, lookup_host};
use tokio::task::JoinSet;

pub async fn serve(listener: UnixListener) -> Result<()> {
    let mut calls = JoinSet::new();
    loop {
        tokio::select! {
            Some(_) = calls.join_next(), if !calls.is_empty() => {},
            incoming = listener.accept(), if calls.len() < 64 => {
                let (stream, _) = incoming?;
                calls.spawn(async move {
                    let _ = tokio::time::timeout(Duration::from_secs(120), forward(stream)).await;
                });
            }
        }
    }
}

async fn forward(mut client: UnixStream) -> Result<()> {
    let mut head = Vec::new();
    while !head.ends_with(b"\r\n\r\n") {
        if head.len() >= 16 * 1024 {
            bail!("Proxy header exceeds limit");
        }
        head.push(client.read_u8().await?);
    }
    let header = std::str::from_utf8(&head)?;
    let mut lines = header.split("\r\n");
    let mut first =
        lines.next().context("Missing proxy request")?.split(' ');
    let method = first.next().context("Missing proxy method")?;
    let target = first.next().context("Missing proxy destination")?;
    if first.next() != Some("HTTP/1.1") || first.next().is_some() {
        bail!("Invalid proxy version");
    }
    let tunnel = method == "CONNECT";
    let url = reqwest::Url::parse(&if tunnel {
        format!("https://{target}/")
    } else {
        target.to_owned()
    })?;
    if (!tunnel && url.scheme() != "http")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        bail!("Invalid proxy URL");
    }
    let host = url
        .host_str()
        .context("Missing proxy host")?
        .trim_matches(['[', ']']);
    let port =
        url.port_or_known_default().context("Missing proxy port")?;
    if !matches!(port, 80 | 443) {
        bail!("Proxy port refused");
    }
    let addresses = tokio::time::timeout(
        Duration::from_secs(5),
        lookup_host((host, port)),
    )
    .await??
    .collect::<Vec<_>>();
    if addresses.is_empty()
        || addresses.iter().any(|address| !public(address.ip()))
    {
        client.write_all(b"HTTP/1.1 403 Forbidden\r\nConnection: close\r\nContent-Length: 0\r\n\r\n").await?;
        return Ok(());
    }
    // Connect to a checked address, never resolve the name again.
    let mut upstream = tokio::time::timeout(
        Duration::from_secs(10),
        TcpStream::connect(addresses.as_slice()),
    )
    .await??;
    if tunnel {
        client
            .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            .await?;
    } else {
        let mut path = url.path().to_owned();
        if let Some(query) = url.query() {
            path.push('?');
            path.push_str(query);
        }
        let mut forwarded = format!(
            "{method} {path} HTTP/1.1\r\nHost: {}\r\nConnection: \
             close\r\n",
            url.authority()
        );
        for line in lines.filter(|line| !line.is_empty()) {
            let (name, _) = line
                .split_once(':')
                .context("Invalid proxy header")?;
            if name.eq_ignore_ascii_case("host")
                || name.eq_ignore_ascii_case("connection")
                || name.to_ascii_lowercase().starts_with("proxy-")
            {
                continue;
            }
            forwarded.push_str(line);
            forwarded.push_str("\r\n");
        }
        forwarded.push_str("\r\n");
        upstream.write_all(forwarded.as_bytes()).await?;
    }
    let (mut client_read, mut client_write) = client.split();
    let (mut upstream_read, mut upstream_write) = upstream.split();
    // Closing either peer closes the upstream socket too, including
    // on cancellation.
    tokio::select! {
        result = tokio::io::copy(&mut client_read, &mut upstream_write) => { result?; }
        result = tokio::io::copy(&mut upstream_read, &mut client_write) => { result?; }
    }
    Ok(())
}

fn public(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => public_v4(address),
        IpAddr::V6(address) => {
            let bits = address.segments();
            // Global unicast only, excluding transition and
            // documentation ranges.
            bits[0] & 0xe000 == 0x2000
                && !(bits[0] == 0x2001
                    && (bits[1] < 0x200 || bits[1] == 0xdb8))
                && bits[0] != 0x2002
                && !(bits[0] == 0x3fff && bits[1] & 0xf000 == 0)
        }
    }
}

fn public_v4(address: Ipv4Addr) -> bool {
    let [a, b, c, _] = address.octets();
    !matches!(
        (a, b, c),
        (0 | 10 | 127 | 224..=255, _, _)
            // GKE reserves 34.118.224.0/20 for internal Services.
            | (34, 118, 224..=239)
            | (100, 64..=127, _)
            | (169, 254, _)
            | (172, 16..=31, _)
            | (192, 168, _)
            | (192, 0, 0 | 2)
            | (192, 88, 99)
            | (198, 18 | 19, _)
            | (198, 51, 100)
            | (203, 0, 113)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_infrastructure_and_alternate_address_spellings() {
        for address in [
            "127.0.0.1",
            "10.2.3.4",
            "34.118.224.0",
            "34.118.226.61",
            "34.118.239.255",
            "169.254.169.254",
            "100.64.1.2",
            "172.31.2.3",
            "192.168.2.3",
            "198.19.1.2",
            "203.0.113.4",
            "224.0.0.1",
            "::1",
            "::ffff:8.8.8.8",
            "fd00::1",
            "fe80::1",
            "2001:db8::1",
            "2002:0808:0808::1",
        ] {
            assert!(
                !public(address.parse().expect("address")),
                "{address}"
            );
        }
        for address in [
            "8.8.4.4",
            "34.118.223.255",
            "34.118.240.0",
            "172.32.0.1",
            "100.128.0.1",
            "2606:4700:4700::1111",
        ] {
            assert!(
                public(address.parse().expect("address")),
                "{address}"
            );
        }
    }
}
