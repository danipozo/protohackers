use tokio::io;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};

async fn socket_echo(mut socket: TcpStream) {
    let mut buf: [u8; 1024] = [0; 1024];

    loop {
        socket.readable().await.unwrap_or(());
        let n_read;
        match socket.try_read(&mut buf) {
            Ok(n) if n == 0 => {
                println!("try_read returned zero: assuming the session is finished");
                return;
            }
            Ok(n) => {
                n_read = n;
                println!("Read {:?} bytes: {:?}", n_read, &buf[0..n_read]);
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                println!("try_read would block: keep waiting");
                continue;
            }
            _ => {
                println!("Unknown error reading socket");
                return;
            }
        };

        if let Err(e) = socket.write_all(&buf[0..n_read]).await {
            eprintln!("Couldn't write to socket: {:?}", e);
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
                tokio::spawn(socket_echo(socket));
            }
            Err(e) => println!("Couldn't accept connection: {:?}", e),
        }
    }
}
