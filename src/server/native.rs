use super::{App, Message, connection_io, rate_allow};
use futures_util::{StreamExt, sink, stream};
use std::{
    io,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::JoinSet,
};

const POLICY_REQUEST: &[u8] = b"<policy-file-request/>\0";
const POLICY: &[u8] = b"<?xml version=\"1.0\"?><cross-domain-policy><allow-access-from domain=\"*\" to-ports=\"2050\" /></cross-domain-policy>\0";

pub(super) async fn serve(a: App, listener: TcpListener) -> io::Result<()> {
    tracing::info!(address=%listener.local_addr()?, "native game socket listening");
    let mut shutdown = a.shutdown.subscribe();
    let mut sessions = JoinSet::new();
    loop {
        tokio::select! {
            _ = shutdown.changed() => break,
            result = sessions.join_next(), if !sessions.is_empty() => {
                result.expect("nonempty native sessions").map_err(io::Error::other)?;
            }
            accepted = listener.accept() => {
                let (socket, peer) = accepted?;
                // Native peers are direct connections, never client-supplied proxy headers.
                if !rate_allow(&mut a.requests.lock().expect("request registry poisoned"), peer.ip(), Instant::now()) {
                    tracing::warn!(%peer, "native connection rate exceeded");
                    continue;
                }
                let Ok(permit) = a.connections.clone().try_acquire_owned() else {
                    tracing::warn!(%peer, "native game connection limit reached");
                    continue;
                };
                let app = a.clone();
                sessions.spawn(async move {
                    let _permit = permit;
                    if let Err(error) = accept(socket, app).await {
                        tracing::warn!(%peer, %error, "native game connection failed");
                    }
                });
            }
        }
    }
    while let Some(result) = sessions.join_next().await {
        result.map_err(io::Error::other)?;
    }
    Ok(())
}

async fn accept(mut socket: TcpStream, a: App) -> io::Result<()> {
    socket.set_nodelay(true)?;
    let mut shutdown = a.shutdown.subscribe();
    let mut first = [0; 4];
    tokio::select! {
        _ = shutdown.changed() => return Ok(()),
        result = tokio::time::timeout(Duration::from_secs(5), socket.read_exact(&mut first)) => { result??; }
    }
    if &first == b"<pol" {
        let mut rest = vec![0; POLICY_REQUEST.len() - 4];
        tokio::select! {
            _ = shutdown.changed() => return Ok(()),
            result = tokio::time::timeout(Duration::from_secs(5), socket.read_exact(&mut rest)) => { result??; }
        }
        if rest != POLICY_REQUEST[4..] {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid native socket policy request",
            ));
        }
        socket.write_all(POLICY).await?;
        return socket.shutdown().await;
    }
    let (read, write) = socket.into_split();
    let input =
        stream::once(async move { Ok::<_, axum::Error>(Message::Binary(first.to_vec().into())) })
            .chain(stream::unfold(read, |mut read| async move {
                let mut bytes = vec![0; 65536];
                match read.read(&mut bytes).await {
                    Ok(0) => None,
                    Ok(n) => {
                        bytes.truncate(n);
                        Some((Ok(Message::Binary(bytes.into())), read))
                    }
                    Err(error) => Some((Err(axum::Error::new(error)), read)),
                }
            }))
            .boxed();
    let output = Box::pin(sink::unfold(write, |mut write, message| async move {
        match message {
            Message::Binary(bytes) => write.write_all(&bytes).await?,
            Message::Close(_) => write.shutdown().await?,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "non-game message on native transport",
                ));
            }
        }
        Ok::<_, io::Error>(write)
    }));
    connection_io(output, input, a, Some("Native SWF".into()), None).await;
    Ok(())
}

#[cfg(test)]
mod tests;
