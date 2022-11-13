use ascii::AsciiString;
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast::{Receiver, Sender};
use tokio_stream::StreamExt;
use tokio_util::codec::{Decoder, FramedRead, LinesCodec, LinesCodecError};

#[derive(Clone, Debug)]
enum Event {
    Msg { user: AsciiString, msg: AsciiString },
    NewUser { user: AsciiString },
    UserLeft { user: AsciiString },
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct AsciiLinesCodec(LinesCodec);

impl AsciiLinesCodec {
    fn new() -> Self {
        AsciiLinesCodec(LinesCodec::new())
    }
}

// Can't implement From if none of the types are defined in my crate
fn std_error_from_lines_codec_error(e: LinesCodecError) -> std::io::Error {
    match e {
        LinesCodecError::MaxLineLengthExceeded => {
            std::io::Error::new(std::io::ErrorKind::Other, "Max line length exceeded")
        }
        LinesCodecError::Io(_e) => _e,
    }
}

impl Decoder for AsciiLinesCodec {
    type Item = AsciiString;
    type Error = std::io::Error;

    fn decode(&mut self, buf: &mut bytes::BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        Ok(self
            .0
            .decode(buf)
            .map_err(std_error_from_lines_codec_error)?
            .map(|x| AsciiString::from_ascii(x))
            .transpose()
            .map_err(|e| {
                Self::Error::new(
                    std::io::ErrorKind::Other,
                    format!(
                        "Invalid ASCII character at position {}",
                        e.ascii_error().valid_up_to()
                    ),
                )
            })?)
    }

    fn decode_eof(&mut self, buf: &mut bytes::BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        Ok(self
            .0
            .decode_eof(buf)
            .map_err(std_error_from_lines_codec_error)?
            .map(|x| AsciiString::from_ascii(x))
            .transpose()
            .map_err(|e| {
                Self::Error::new(
                    std::io::ErrorKind::Other,
                    format!(
                        "Invalid ASCII character at position {}",
                        e.ascii_error().valid_up_to()
                    ),
                )
            })?)
    }
}

fn valid_name(name: &AsciiString) -> bool {
    name.len() >= 1 && name.chars().all(|c| c.is_ascii_alphanumeric())
}

async fn process_socket(
    socket: TcpStream,
    user_db: Arc<Mutex<BTreeSet<AsciiString>>>,
    tx: Sender<Event>,
) {
    let (rd, mut wr) = tokio::io::split(socket);
    let mut line_delimited = FramedRead::new(rd, AsciiLinesCodec::new());

    // Read username
    wr.write_all(b"Welcome to budgetchat! What shall I call you?\n")
        .await;
    let name = match line_delimited.next().await {
        Some(Ok(n)) => n,
        None => {
            println!("Connection closed while reading username");
            return;
        }
        Some(Err(e)) => {
            println!("Error reading username: {}", e);
            return;
        }
    };

    let name_inserted;
    let user_list: AsciiString;
    if let Ok(ref mut s) = user_db.lock() {
        let name_exists = s.contains::<AsciiString>(&name);
        if valid_name(&name) && !name_exists {
            // Add user to user list
            s.insert(name.clone());
            // Presence notification
            name_inserted = Some(true);
        } else {
            name_inserted = Some(false);
        }

        user_list = s
            .iter()
            .map(|x| x.clone())
            .filter(|x| x != &name)
            .reduce(|a, b| a.clone() + &AsciiString::from_ascii(", ").unwrap() + &b)
            .unwrap_or(AsciiString::from_ascii("").unwrap());
    } else {
        println!("Error accessing user list");
        return;
    };

    let mut rx = if let Some(true) = name_inserted {
        tx.send(Event::NewUser { user: name.clone() });
        let rx = tx.subscribe();
        wr.write_all(format!("* The room contains: {}\n", user_list).as_bytes())
            .await;
        rx
    } else if let Some(false) = name_inserted {
        wr.write_all(b"Illegal username\n").await;
        return;
    } else {
        println!("Something was messed up and the name was not inserted nor rejected");
        return;
    };

    // Main event loop
    loop {
        tokio::select! {
            ev = rx.recv() => {
                let ev = if let Ok(e) = ev { e } else { return; };
                match ev {
                    Event::Msg { user: u, msg: m } => {
                        if u != name {
                            wr.write_all(format!("[{u}] {m}\n").as_bytes()).await;
                        }
                    },
                    Event::NewUser { user: u } => {
                        if u != name {
                            wr.write_all(format!("* {u} has entered the room\n").as_bytes()).await;
                        }
                    },
                    Event::UserLeft { user: u } => {
                        if u != name {
                            wr.write_all(format!("* {u} has left the room\n").as_bytes()).await;
                        }
                    }
                }
            },
            m = line_delimited.next() => {
                if let Some(m) = m {
                    match m {
                        Ok(m) => {
                            tx.send(Event::Msg{ user: name.clone(), msg: m});
                        },
                        Err(e) => {
                            println!("Error reading message: {}", e);
                        }
                    }
                } else {
                    user_db
                        .lock()
                        .unwrap_or_else(|e| panic!("Error locking user list: {}", e))
                        .take(&name);
                    tx.send(Event::UserLeft { user: name.clone() });
                    return;
                }
            },
        }
    }
}

#[tokio::main]
async fn main() {
    let listener = TcpListener::bind("0.0.0.0:39456").await.unwrap();
    let (tx, _rx) = tokio::sync::broadcast::channel(1000);

    let user_db: Arc<Mutex<BTreeSet<AsciiString>>> = Arc::new(Mutex::new(BTreeSet::new()));

    loop {
        match listener.accept().await {
            Ok((socket, addr)) => {
                println!("Accepted connection from {:?}", addr);
                tokio::spawn(process_socket(
                    socket,
                    user_db.clone(),
                    tx.clone(),
                ));
            }
            Err(e) => println!("Couldn't accept connection: {:?}", e),
        }
    }
}
