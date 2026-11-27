//! A stand-in for `openvscode-server` in the `web_ide` test: it
//! listens on the port it is given and answers every connection with
//! an empty page, until it is ended.

use std::io::{Read, Write};
use std::net::TcpListener;

fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    let port = args
        .iter()
        .position(|arg| arg == "--port")
        .and_then(|at| args.get(at + 1))
        .expect("a --port");
    let listener = TcpListener::bind(format!("127.0.0.1:{port}"))
        .expect("the port is free");
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else {
            continue;
        };
        let mut request = [0; 1024];
        let _ = stream.read(&mut request);
        let _ = stream.write_all(
            b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
        );
    }
}
