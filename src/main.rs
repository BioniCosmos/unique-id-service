use std::{
    collections::HashSet,
    env, error,
    fs::File,
    io::{BufRead, BufReader, Write as _},
    net::UdpSocket,
    path::Path,
    str::Utf8Error,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow};
use nanoid::{alphabet::HEX_LOWERCASE, nanoid};

fn main() -> Result<()> {
    let mut store = File::options()
        .create(true)
        .append(true)
        .read(true)
        .open(
            Path::new(
                &env::var("STATE_DIRECTORY")
                    .context("failed to read environment variable `STATE_DIRECTORY`")?,
            )
            .join("store"),
        )
        .context("failed to open the store file")?;
    let mut ids = BufReader::new(&store)
        .lines()
        .collect::<Result<_, _>>()
        .context("failed to read store")?;

    let addr = env::var("LISTEN").unwrap_or("127.0.0.1:3000".to_string());
    let socket = UdpSocket::bind(addr).context("failed to bind")?;
    let mut buf = [0; 512];
    loop {
        if let Err(e) = handle(&socket, &mut buf, &mut ids, &mut store) {
            eprintln!("{e:?}");
        }
    }
}

#[inline]
fn handle(
    socket: &UdpSocket,
    buf: &mut [u8],
    ids: &mut HashSet<String>,
    store: &mut File,
) -> Result<()> {
    let (n, _) = socket.recv_from(buf).context("failed to receive")?;
    let message = deserialize(&buf[..n])?;
    dbg!(message);
    generate_id(ids, store)?;
    Ok(())
}

fn generate_id(ids: &mut HashSet<String>, store: &mut File) -> Result<String> {
    let end = Instant::now() + Duration::from_secs(2);
    while Instant::now() <= end {
        let id = nanoid!(1, &HEX_LOWERCASE);
        if ids.insert(id.clone()) {
            writeln!(store, "{id}").context("failed to write to store")?;
            store.sync_data().context("failed to sync to store")?;
            return Ok(id);
        }
    }
    Err(anyhow!("generation time exceeded"))
}

#[derive(Debug)]
#[allow(unused)]
struct Message {
    header: Header,
    question: Question,
}

#[derive(Debug)]
#[allow(unused)]
struct Header {
    id: u16,
    qr: Type,
    opcode: u8,
    aa: bool,
    tc: bool,
    rd: bool,
    ra: bool,
    rcode: u8,
    qdcount: u16,
    ancount: u16,
    nscount: u16,
    arcount: u16,
}

#[derive(Debug)]
enum Type {
    Query,
    Response,
}

#[derive(Debug)]
#[allow(unused)]
struct Question {
    qname: String,
    qtype: u16,
    qclass: u16,
}

#[derive(Debug)]
enum Error {
    InvalidDomain(Utf8Error),
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Error::InvalidDomain(e) => Some(e),
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::InvalidDomain(_) => write!(f, "invalid domain name in `QNAME`"),
        }
    }
}

fn deserialize(raw: &[u8]) -> Result<Message, Error> {
    macro_rules! from_bytes {
        ($range:expr) => {
            u16::from_be_bytes(*raw[$range].as_array().unwrap())
        };
    }

    let header = Header {
        id: from_bytes!(..2),
        qr: if raw[2] >> 7 == 0 {
            Type::Query
        } else {
            Type::Response
        },
        opcode: raw[2] >> 3 & 0b1111,
        aa: raw[2] >> 2 & 1 == 1,
        tc: raw[2] >> 1 & 1 == 1,
        rd: raw[2] & 1 == 1,
        ra: raw[3] >> 7 & 1 == 1,
        rcode: raw[3] & 0b1111,
        qdcount: from_bytes!(4..6),
        ancount: from_bytes!(6..8),
        nscount: from_bytes!(8..10),
        arcount: from_bytes!(10..12),
    };

    let mut i = 12;
    let question = Question {
        qname: {
            let mut segments = vec![];
            loop {
                let len = raw[i] as usize;
                i += 1;
                if len == 0 {
                    break;
                }
                segments.push(str::from_utf8(&raw[i..i + len]).map_err(Error::InvalidDomain)?);
                i += len;
            }
            segments.join(".")
        },
        qtype: from_bytes!(i..i + 2),
        qclass: {
            i += 2;
            from_bytes!(i..i + 2)
        },
    };

    Ok(Message { header, question })
}
