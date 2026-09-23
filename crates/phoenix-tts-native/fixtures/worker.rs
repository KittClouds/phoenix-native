//! Scripted child process for failure-path tests. Never used for inference proof.
use phoenix_tts_native::wire::*;
use std::{
    io::{Read, Write},
    net::TcpStream,
    thread,
    time::Duration,
};
fn read(s: &mut TcpStream) -> Header {
    let mut b = [0; HEADER];
    s.read_exact(&mut b).unwrap();
    Header::decode(b).unwrap()
}
fn emit(s: &mut TcpStream, h: Header) {
    s.write_all(&h.encode()).unwrap();
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let mut s = TcpStream::connect(format!("127.0.0.1:{}", args[2])).unwrap();
    let nonce: Vec<u8> = args[3]
        .as_bytes()
        .chunks_exact(2)
        .map(|x| u8::from_str_radix(std::str::from_utf8(x).unwrap(), 16).unwrap())
        .collect();
    s.write_all(&nonce).unwrap();
    if std::fs::read(&args[1]).unwrap() == b"startup stall" {
        thread::sleep(Duration::from_secs(30));
    }
    emit(
        &mut s,
        Header {
            kind: READY,
            request: 0,
            sequence: 0,
            value: 24000,
        },
    );
    loop {
        let h = read(&mut s);
        let mut lens = [0; 8];
        s.read_exact(&mut lens).unwrap();
        let n = u32::from_le_bytes(lens[..4].try_into().unwrap()) as usize;
        let d = u32::from_le_bytes(lens[4..].try_into().unwrap()) as usize;
        let mut text = vec![0; n];
        s.read_exact(&mut text).unwrap();
        let mut direction = vec![0; d];
        s.read_exact(&mut direction).unwrap();
        if h.kind == VOICED_REQUEST {
            let mut size = [0u8; 4];
            s.read_exact(&mut size).unwrap();
            let size = u32::from_le_bytes(size) as usize;
            assert!(size <= 512 * 1024);
            let mut voice = vec![0; size];
            s.read_exact(&mut voice).unwrap();
            assert_eq!(&voice[..4], b"BRZV");
        } else {
            assert_eq!(h.kind, REQUEST);
        }
        let mode = std::str::from_utf8(&text).unwrap();
        if mode == "prefill" {
            thread::sleep(Duration::from_secs(30));
        }
        if mode == "reject" {
            emit(
                &mut s,
                Header {
                    kind: FAILED,
                    request: h.request,
                    sequence: 0,
                    value: 1,
                },
            );
            continue;
        }
        emit(
            &mut s,
            Header {
                kind: STARTED,
                request: h.request,
                sequence: 0,
                value: 128,
            },
        );
        if mode == "crash" {
            std::process::exit(7);
        }
        let request = if mode == "stale" {
            h.request + 1
        } else {
            h.request
        };
        let frames = if mode == "oversize" { 2049 } else { 4 };
        emit(
            &mut s,
            Header {
                kind: AUDIO,
                request,
                sequence: 1,
                value: frames,
            },
        );
        if mode == "partial" {
            s.write_all(&[1, 0]).unwrap();
            std::process::exit(8);
        }
        s.write_all(&[1, 0, 2, 0, 3, 0, 4, 0]).unwrap();
        if mode == "stall" {
            thread::sleep(Duration::from_secs(30));
        }
        if mode == "eof" {
            std::process::exit(0);
        }
        emit(
            &mut s,
            Header {
                kind: if mode == "limit" { LIMIT } else { EOS },
                request: h.request,
                sequence: 2,
                value: if mode == "count" { 5 } else { 4 },
            },
        );
        let _barrier = read(&mut s);
        emit(
            &mut s,
            Header {
                kind: if mode == "late" { AUDIO } else { QUIET },
                request: h.request,
                sequence: 3,
                value: 4,
            },
        );
    }
}
