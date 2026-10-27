use std::{
    collections::HashSet,
    env,
    fs::File,
    io::{self, BufRead, BufReader, Write as _},
    net::UdpSocket,
    path::Path,
    str::Utf8Error,
    time::{Duration, Instant},
};

use anyhow::{Context as _, Result, anyhow};
use nanoid::{alphabet::HEX_LOWERCASE, nanoid};

fn main() -> Result<()> {
    let mut store = File::options()
        .create(true)
        .append(true)
        .read(true)
        .open(Path::new(&env::var("STATE_DIRECTORY").unwrap_or_default()).join("store"))
        .context("failed to open the store file")?;
    let mut ids = BufReader::new(&store)
        .lines()
        .collect::<Result<_, _>>()
        .context("failed to read store")?;

    let addr = env::var("LISTEN").unwrap_or("127.0.0.1:3000".to_string());
    let socket = UdpSocket::bind(addr).context("failed to bind")?;
    println!("binding on `{}`", socket.local_addr().unwrap());

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
) -> Result<(), Error> {
    macro_rules! res_header {
        ($header:expr, $rcode:expr, $qdcount:expr, $ancount:expr) => {
            Header {
                id: $header.id,
                qr: Type::Response,
                opcode: $header.opcode,
                aa: true,
                tc: false,
                rd: $header.rd,
                ra: false,
                rcode: $rcode,
                qdcount: $qdcount,
                ancount: $ancount,
                nscount: 0,
                arcount: 0,
            }
        };
    }

    let (n, addr) = socket.recv_from(buf).map_err(Error::Receive)?;

    let Message {
        header, question, ..
    } = match deserialize(&buf[..n]) {
        Ok(x) => x,
        Err(e @ Error::InvalidDomain(_, header)) => {
            let (n, res) = serialize(&Message {
                header: res_header!(header, 1, 0, 0),
                question: None,
                answer: None,
            })
            .unwrap();
            if let Err(e) = socket.send_to(&res[..n], addr) {
                eprintln!("failed to send back error response after error occurred: {e}");
            }
            return Err(e);
        }
        _ => unreachable!(),
    };

    if header.qr != Type::Query {
        return Err(Error::InvalidType);
    }
    if header.opcode != 0 {
        let (n, res) = serialize(&Message {
            header: res_header!(header, 4, 1, 0),
            question,
            answer: None,
        })
        .unwrap();
        if let Err(e) = socket.send_to(&res[..n], addr) {
            eprintln!("failed to send back error response after error occurred: {e}");
        }
        return Err(Error::UnsupportedQuery(header.opcode));
    }

    let question = question.unwrap();
    if question.qname != "id" {
        let (n, res) = serialize(&Message {
            header: res_header!(header, 3, 1, 0),
            question: Some(question),
            answer: None,
        })
        .unwrap();
        return socket
            .send_to(&res[..n], addr)
            .and(Ok(()))
            .map_err(Error::Send);
    }
    if question.qtype != 16 {
        let (n, res) = serialize(&Message {
            header: res_header!(header, 0, 1, 0),
            question: Some(question),
            answer: None,
        })
        .unwrap();
        return socket
            .send_to(&res[..n], addr)
            .and(Ok(()))
            .map_err(Error::Send);
    }

    let id = generate_id(ids, store)?;
    let (n, res) = serialize(&Message {
        header: res_header!(header, 0, 1, 1),
        question: Some(question.clone()),
        answer: Some(Record {
            name: question.qname,
            r#type: 16,
            class: question.qclass,
            ttl: 0,
            rdlength: (id.len() + 1) as u16,
            rdata: id,
        }),
    })
    .unwrap();
    socket
        .send_to(&res[..n], addr)
        .and(Ok(()))
        .map_err(Error::Send)
}

fn generate_id(ids: &mut HashSet<String>, store: &mut File) -> Result<String, Error> {
    let end = Instant::now() + Duration::from_secs(2);
    while Instant::now() <= end {
        let id = nanoid!(10, &HEX_LOWERCASE);
        if ids.insert(id.clone()) {
            writeln!(store, "{id}").map_err(Error::StoreWrite)?;
            store.sync_data().map_err(Error::StoreSync)?;
            return Ok(id);
        }
    }
    Err(Error::PoolFull)
}

#[derive(Debug)]
#[allow(unused)]
struct Message {
    header: Header,
    question: Option<Question>,
    answer: Option<Record>,
}

#[derive(Copy, Clone, Debug)]
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

#[derive(Copy, Clone, Debug, PartialEq)]
enum Type {
    Query,
    Response,
}

#[derive(Clone, Debug)]
#[allow(unused)]
struct Question {
    qname: String,
    qtype: u16,
    qclass: u16,
}

#[derive(Debug)]
struct Record {
    name: String,
    r#type: u16,
    class: u16,
    ttl: u32,
    rdlength: u16,
    rdata: String,
}

#[derive(Debug)]
enum Error {
    Receive(io::Error),
    Send(io::Error),
    InvalidDomain(Utf8Error, Header),
    InvalidType,
    UnsupportedQuery(u8),
    StoreWrite(io::Error),
    StoreSync(io::Error),
    PoolFull,
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        use Error::*;
        match self {
            Receive(e) => Some(e),
            Send(e) => Some(e),
            InvalidDomain(e, ..) => Some(e),
            InvalidType => None,
            UnsupportedQuery(_) => None,
            StoreWrite(e) => Some(e),
            StoreSync(e) => Some(e),
            PoolFull => None,
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use Error::*;
        match self {
            Receive(_) => write!(f, "failed to receive message"),
            Send(_) => write!(f, "failed to send message"),
            InvalidDomain(..) => write!(f, "invalid domain name in `QNAME`"),
            InvalidType => write!(f, "Invalid message type/`QR`: the request is a “response”."),
            UnsupportedQuery(opcode) => write!(f, "unsupported query type/`OPCODE`: `{opcode}`"),
            StoreWrite(_) => write!(f, "failed to write to store"),
            StoreSync(_) => write!(f, "failed to sync to store"),
            PoolFull => write!(f, "generation time exceeded"),
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
                segments.push(
                    str::from_utf8(&raw[i..i + len])
                        .map_err(|e| Error::InvalidDomain(e, header))?,
                );
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

    Ok(Message {
        header,
        question: Some(question),
        answer: None,
    })
}

fn serialize(
    Message {
        header,
        question,
        answer,
    }: &Message,
) -> Result<(usize, [u8; 512])> {
    let mut buf = [0; 512];
    buf[..2].copy_from_slice(&header.id.to_be_bytes());
    buf[2] = header.rd as u8;
    buf[2] |= (header.tc as u8) << 1;
    buf[2] |= (header.aa as u8) << 2;
    buf[2] |= header.opcode << 3;
    buf[2] |= (header.qr as u8) << 7;
    buf[3] = header.rcode;
    buf[3] |= (header.ra as u8) << 7;
    buf[4..6].copy_from_slice(&header.qdcount.to_be_bytes());
    buf[6..8].copy_from_slice(&header.ancount.to_be_bytes());

    let mut i = 12;

    if header.qdcount == 1 {
        let question = question.as_ref().unwrap();
        for segment in question
            .qname
            .split('.')
            .filter(|segment| !segment.is_empty())
        {
            buf[i] = u8::try_from(segment.len())
                .map_err(|_| anyhow!("serializing: max domain name segment length exceeded"))?;
            i += 1;
            buf[i..i + segment.len()].copy_from_slice(segment.as_bytes());
            i += segment.len();
        }
        buf[i] = 0;
        i += 1;
        buf[i..i + 2].copy_from_slice(&question.qtype.to_be_bytes());
        i += 2;
        buf[i..i + 2].copy_from_slice(&question.qclass.to_be_bytes());
        i += 2;
    }

    if header.ancount > 0 {
        let answer = answer.as_ref().unwrap();
        for segment in answer.name.split('.').filter(|segment| !segment.is_empty()) {
            buf[i] = u8::try_from(segment.len())
                .map_err(|_| anyhow!("serializing: max domain name segment length exceeded"))?;
            i += 1;
            buf[i..i + segment.len()].copy_from_slice(segment.as_bytes());
            i += segment.len();
        }
        buf[i] = 0;
        i += 1;
        buf[i..i + 2].copy_from_slice(&answer.r#type.to_be_bytes());
        i += 2;
        buf[i..i + 2].copy_from_slice(&answer.class.to_be_bytes());
        i += 2;
        buf[i..i + 4].copy_from_slice(&answer.ttl.to_be_bytes());
        i += 4;
        buf[i..i + 2].copy_from_slice(&answer.rdlength.to_be_bytes());
        i += 2;
        buf[i] =
            u8::try_from(answer.rdata.len()).map_err(|_| anyhow!("max `RDATA` length exceeded"))?;
        i += 1;
        buf[i..i + answer.rdata.len()].copy_from_slice(answer.rdata.as_bytes());
        i += answer.rdata.len();
    }

    Ok((i, buf))
}
