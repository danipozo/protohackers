use bytes::{Buf, BytesMut};
use futures::sink::SinkExt;
use std::collections::BTreeMap;
use std::ops::Bound::Included;
use tokio::net::{TcpListener, TcpStream};
use tokio_stream::StreamExt;
use tokio_util::codec::{Decoder, Encoder, FramedRead, FramedWrite};

#[derive(Debug)]
enum AssetProtoRequest {
    Insert { timestamp: i32, price: i32 },
    Query { beginning: i32, end: i32 },
}
enum AssetProtoResponse {
    PeriodMean(i32),
    ErrorResponse(String),
}
#[derive(Debug)]
enum AssetProtoError {
    WrongMessageType(u8),
    IOError(std::io::Error),
}

impl From<std::io::Error> for AssetProtoError {
    fn from(e: std::io::Error) -> Self {
        Self::IOError(e)
    }
}

struct AssetProtoCodec;

impl Decoder for AssetProtoCodec {
    type Item = AssetProtoRequest;
    type Error = AssetProtoError;

    fn decode(&mut self, src: &mut bytes::BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if src.len() < 9 {
            return Ok(None);
        }

        let data = src[0..9].to_vec();
        src.advance(9);

        let msg_type = data[0];
        let mut bytes_array = [0u8; 4];
        bytes_array.copy_from_slice(&data[1..5]);
        let first_int = i32::from_be_bytes(bytes_array);
        bytes_array.copy_from_slice(&data[5..9]);
        let second_int = i32::from_be_bytes(bytes_array);
        match msg_type as char {
            'I' => {
                Ok(Some(AssetProtoRequest::Insert {
                    timestamp: first_int,
                    price: second_int,
                }))
            }
            'Q' => {
                Ok(Some(AssetProtoRequest::Query {
                    beginning: first_int,
                    end: second_int,
                }))
            }
            _ => Err(AssetProtoError::WrongMessageType(msg_type)),
        }
    }
}

impl Encoder<AssetProtoResponse> for AssetProtoCodec {
    type Error = std::io::Error;

    fn encode(&mut self, item: AssetProtoResponse, dst: &mut BytesMut) -> Result<(), Self::Error> {
        match item {
            AssetProtoResponse::PeriodMean(m) => {
                dst.extend_from_slice(&m.to_be_bytes());
                Ok(())
            }
            AssetProtoResponse::ErrorResponse(s) => {
                dst.extend_from_slice(("Error: ".to_owned() + &s).as_bytes());
                Err(std::io::Error::new(std::io::ErrorKind::Other, s))
            }
        }
    }
}

async fn process_socket(socket: TcpStream) {
    let (rd, wr) = tokio::io::split(socket);

    let mut prices = BTreeMap::new();

    let mut deserialized = FramedRead::new(rd, AssetProtoCodec);
    let mut serialized = FramedWrite::new(wr, AssetProtoCodec);
    while let Some(value) = deserialized.next().await {
        println!("Starting service iteration for value: {:?}", value);
        let value = match value {
            Ok(v) => v,
            Err(e) => {
                println!("Error parsing value: {:?}", e);
                serialized
                    .send(AssetProtoResponse::ErrorResponse(
                        "Malformed request (error parsing value)".to_owned(),
                    ))
                    .await
                    .unwrap_or(());
                return;
            }
        };

        match value {
            AssetProtoRequest::Insert { timestamp, price } => {
                prices.insert(timestamp, price);
            }
            AssetProtoRequest::Query { beginning, end } => {
                let mean = if beginning <= end {
                    prices
                        .range((Included(beginning), Included(end)))
                        .map(|(_k, v)| v)
                        .zip(1..)
                        .fold(0., |s, (e, i)| (*e as f64 + s * (i - 1) as f64) / i as f64)
                } else {
                    0f64
                };
                let mean = mean.round().clamp(i32::MIN as f64, i32::MAX as f64) as i32;
                serialized
                    .send(AssetProtoResponse::PeriodMean(mean))
                    .await
                    .unwrap_or(());
            }
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
