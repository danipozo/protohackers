use num_integer::Roots;
use serde_json;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio_serde;
use tokio_stream::StreamExt;
use tokio_util::codec::{Decoder, FramedRead, LinesCodec, LinesCodecError};

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct BytesLinesCodec(LinesCodec);

impl BytesLinesCodec {
    fn new() -> Self {
        BytesLinesCodec(LinesCodec::new())
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

impl Decoder for BytesLinesCodec {
    type Item = bytes::BytesMut;
    type Error = std::io::Error;

    fn decode(&mut self, buf: &mut bytes::BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        Ok(self
            .0
            .decode(buf)
            .map_err(|e| std_error_from_lines_codec_error(e))?
            .map(|x| x.as_bytes().into()))
    }

    fn decode_eof(&mut self, buf: &mut bytes::BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        Ok(self
            .0
            .decode_eof(buf)
            .map_err(|e| std_error_from_lines_codec_error(e))?
            .map(|x| x.as_bytes().into()))
    }
}

fn is_prime(i: u64) -> bool {
    match i {
        0 => false,
        1 => false,
        _ => (2..=i.sqrt())
            .into_iter()
            .all(|x| i.rem_euclid(x) != 0 || i == x),
    }
}

fn is_valid_prime(i: &serde_json::value::Number) -> bool {
    if let Some(i) = i.as_i64() {
        if i < 0 {
            return false;
        }
        return is_prime(i.abs_diff(0));
    }
    if let Some(i) = i.as_u64() {
        return is_prime(i);
    }
    return false;
}

async fn process_socket(socket: TcpStream) {
    let (rd, mut wr) = tokio::io::split(socket);

    let length_delimited = FramedRead::new(rd, BytesLinesCodec::new());
    let mut deserialized = tokio_serde::SymmetricallyFramed::new(
        length_delimited,
        tokio_serde::formats::SymmetricalJson::<serde_json::Value>::default(),
    );

    while let Some(value) = deserialized.next().await {
        println!("Starting service iteration for value: {:?}", value);
        let value = match value {
            Ok(v) => v,
            Err(e) => {
                println!("Error parsing value: {:?}", e);
                wr.write_all(b"{\"error\": \"Malformed request (error parsing value)\"}")
                    .await
                    .unwrap_or(());
                return;
            }
        };

        let method = value.get("method");
        let number = value.get("number");
        if !(value.is_object() && method.is_some() && number.is_some())
            || method.unwrap_or(&serde_json::Value::Null)
                != &serde_json::Value::String("isPrime".to_owned())
        {
            wr.write_all(b"{\"error\": \"Malformed request (missing or incorrect member in response)\"}")
                .await
                .unwrap_or(());
            return;
        }

        if let serde_json::Value::Number(n) = number.unwrap() {
            println!("Returning response for number: {}", n);
            let response = serde_json::json!({"method": "isPrime", "prime": is_valid_prime(&n)})
                .to_string()
                + "\n";
            wr.write_all(response.as_bytes()).await.unwrap_or(());
        } else {
            wr.write_all(b"{\"error\": \"Malformed request (no number)\"}")
                .await
                .unwrap_or(());
            return;
        }
    }
}

#[tokio::main]
async fn main() {
    let listener = TcpListener::bind("0.0.0.0:39456").await.unwrap();

    loop {
        match listener.accept().await {
            Ok((socket, addr)) => {
                println!("Accepted connection from {:?}", addr);
                tokio::spawn(process_socket(socket));
            }
            Err(e) => println!("Couldn't accept connection: {:?}", e),
        }
    }
}
