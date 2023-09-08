use std::{net::SocketAddr, sync::Arc, time::Duration};

use anyhow::{Context, Result};
use quinn::{Connecting, Endpoint, SendStream};
use rkvm_protocol::Packet;
use tokio::io::{AsyncWrite, AsyncWriteExt, BufWriter};
use tracing::Instrument;

lazy_static::lazy_static! {
    static ref MOUSE_CHANNEL: tokio::sync::broadcast::Sender<Arc<[u8]>> = {
        let (tx, _) = tokio::sync::broadcast::channel(120);
        tx
    };

    static ref KEYBOARD_CHANNEL: tokio::sync::broadcast::Sender<Arc<[u8]>> = {
        let (tx, _) = tokio::sync::broadcast::channel(30);
        tx
    };

    static ref MISC_CHANNEL: tokio::sync::broadcast::Sender<Arc<[u8]>> = {
        let (tx, _) = tokio::sync::broadcast::channel(30);
        tx
    };
}

async fn write_packet<W: AsyncWrite + Unpin>(writer: &mut W, packet: &[u8]) -> Result<()> {
    writer.write_u32(packet.len() as u32).await?;
    writer.write_all(packet).await?;
    writer.flush().await?;

    Ok(())
}

pub async fn sender(mut rx: tokio::sync::mpsc::Receiver<Packet>) {
    while let Some(packet) = rx.recv().await {
        if packet.event.is_high_freq() {
            log::trace!("Sending event {}: {:?}", packet.id, packet.event);
        } else {
            log::debug!("Sending event {}: {:?}", packet.id, packet.event);
        }

        match packet.event.kind() {
            rkvm_protocol::EventKind::Mouse => {
                let _ = MOUSE_CHANNEL.send(packet.to_vec().into());
            }
            rkvm_protocol::EventKind::Keyboard => {
                let _ = KEYBOARD_CHANNEL.send(packet.to_vec().into());
            }
            rkvm_protocol::EventKind::Misc => {
                let _ = MISC_CHANNEL.send(packet.to_vec().into());
            }
        }
    }
}

async fn tx_task(
    conn: SendStream,
    mut sub: tokio::sync::broadcast::Receiver<Arc<[u8]>>,
) -> Result<()> {
    let mut conn = BufWriter::new(conn);

    while let Ok(packet) = sub.recv().await {
        write_packet(&mut conn, &packet).await?;
    }

    Ok(())
}

async fn handle_conn(conn: Connecting) -> Result<()> {
    let conn = conn.await?;

    let span = tracing::info_span!(
        "connection",
        remote = %conn.remote_address(),
        id = %conn.stable_id(),
    );
    let _guard = span.enter();

    log::info!("New connection");

    let mouse_tx = conn.open_uni().await.context("Open mouse tx")?;
    mouse_tx.set_priority(2)?;
    tokio::spawn(async move {
        let sub = MOUSE_CHANNEL.subscribe();

        if let Err(e) = tx_task(mouse_tx, sub).await {
            log::error!("Error handling mouse tx: {}", e);
        }
    }.in_current_span());

    let keyboard_tx = conn.open_uni().await.context("Open keyboard tx")?;
    keyboard_tx.set_priority(1)?;
    tokio::spawn(async move {
        let sub = KEYBOARD_CHANNEL.subscribe();

        if let Err(e) = tx_task(keyboard_tx, sub).await {
            log::error!("Error handling keyboard tx: {}", e);
        }
    }.in_current_span());

    let misc_tx = conn.open_uni().await.context("Open misc tx")?;
    misc_tx.set_priority(0)?;
    tokio::spawn(async move {
        let sub = MISC_CHANNEL.subscribe();

        if let Err(e) = tx_task(misc_tx, sub).await {
            log::error!("Error handling misc tx: {}", e);
        }
    }.in_current_span());

    let reason = conn.closed().await;
    log::info!("Connection closed: {:?}", reason);

    Ok(())
}

pub async fn server() -> Result<()> {
    let (endpoint, _server_cert) = make_server_endpoint("0.0.0.0:12334".parse()?)?;

    loop {
        let conn = if let Some(conn) = endpoint.accept().await {
            conn
        } else {
            return Ok(());
        };

        tokio::spawn(async move {
            if let Err(e) = handle_conn(conn).await {
                log::error!("Error handling connection: {}", e);
            }
        });
    }
}

fn make_server_endpoint(bind_addr: SocketAddr) -> Result<(Endpoint, Vec<u8>)> {
    let (server_config, server_cert) = configure_server()?;
    let endpoint = Endpoint::server(server_config, bind_addr)?;
    Ok((endpoint, server_cert))
}

/// Returns default server configuration along with its certificate.
fn configure_server() -> Result<(quinn::ServerConfig, Vec<u8>)> {
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let cert_der = cert.serialize_der().unwrap();
    let priv_key = cert.serialize_private_key_der();
    let priv_key = rustls::PrivateKey(priv_key);
    let cert_chain = vec![rustls::Certificate(cert_der.clone())];

    let mut server_config = quinn::ServerConfig::with_single_cert(cert_chain, priv_key)?;
    let transport_config = Arc::get_mut(&mut server_config.transport).unwrap();
    transport_config.keep_alive_interval(Some(Duration::from_secs(5)));
    transport_config.max_idle_timeout(Some(Duration::from_secs(10).try_into()?));
    // transport_config.max_concurrent_uni_streams(100u8.into());
    // transport_config.max_concurrent_bidi_streams(100u8.into());

    Ok((server_config, cert_der))
}
