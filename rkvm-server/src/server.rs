use std::net::SocketAddr;

use anyhow::Result;
use rkvm_protocol::Packet;
use tokio::{
    io::{AsyncWriteExt, BufWriter},
    net::TcpStream,
};

lazy_static::lazy_static! {
    static ref BCAST_CHANNEL: tokio::sync::broadcast::Sender<Vec<u8>> = {
        let (tx, _) = tokio::sync::broadcast::channel(120);
        tx
    };
}

pub async fn sender(mut rx: tokio::sync::mpsc::Receiver<Packet>) {
    while let Some(packet) = rx.recv().await {
        if packet.event.is_high_freq() {
            log::trace!("Sending event {}: {:?}", packet.id, packet.event);
        } else {
            log::debug!("Sending event {}: {:?}", packet.id, packet.event);
        }
        
        let _ = BCAST_CHANNEL.send(packet.to_vec());
    }
}

async fn handle_conn(conn: TcpStream, addr: SocketAddr) -> Result<()> {
    log::info!("New connection from {}", addr);

    let _ = conn.set_nodelay(true);

    let mut conn = BufWriter::new(conn);

    let mut sub = BCAST_CHANNEL.subscribe();

    while let Ok(event) = sub.recv().await {
        conn.write_u32(event.len() as u32).await?;
        conn.write_all(&event).await?;
        conn.flush().await?;
    }

    Ok(())
}

pub async fn server() -> Result<()> {
    let listener = tokio::net::TcpListener::bind("0.0.0.0:12333").await?;

    loop {
        let (conn, addr) = listener.accept().await?;

        tokio::spawn(async move {
            if let Err(e) = handle_conn(conn, addr).await {
                log::error!("Error handling connection: {}", e);
            }
        });
    }
}
