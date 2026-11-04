use anyhow::{Context, Result, ensure};
use std::{io::Read, time::Duration};
use tokio::sync::mpsc;

const MESSAGE: &[u8] = b"stop\n";
const MESSAGE_DEADLINE: Duration = Duration::from_secs(2);

pub(super) fn shutdown() -> Result<impl Future<Output = Result<()>>> {
    let (sender, receiver) = mpsc::channel(MESSAGE.len());
    // A dedicated thread keeps an idle parent pipe out of Tokio's blocking pool at shutdown.
    std::thread::Builder::new()
        .name("desktop-parent".into())
        .spawn(move || read(std::io::stdin().lock(), sender))?;
    Ok(request(receiver))
}

fn read(mut input: impl Read, sender: mpsc::Sender<std::io::Result<u8>>) {
    for _ in 0..MESSAGE.len() {
        let mut byte = [0];
        let result = loop {
            match input.read(&mut byte) {
                Ok(0) => return,
                Ok(_) => break Ok(byte[0]),
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => break Err(error),
            }
        };
        let failed = result.is_err();
        if sender.blocking_send(result).is_err() || failed {
            return;
        }
    }
}

async fn request(mut receiver: mpsc::Receiver<std::io::Result<u8>>) -> Result<()> {
    let Some(first) = receiver.recv().await else {
        tracing::info!(reason = "parent_eof", "desktop shutdown requested");
        return Ok(());
    };
    ensure!(
        first.context("read desktop parent pipe")? == MESSAGE[0],
        "invalid desktop command; expected exactly stop\\n"
    );
    tokio::time::timeout(MESSAGE_DEADLINE, async {
        for expected in &MESSAGE[1..] {
            let byte = receiver
                .recv()
                .await
                .context("incomplete desktop command before parent EOF")??;
            ensure!(
                byte == *expected,
                "invalid desktop command; expected exactly stop\\n"
            );
        }
        Ok::<_, anyhow::Error>(())
    })
    .await
    .context("desktop command deadline exceeded (2 seconds)")??;
    tracing::info!(reason = "stop", "desktop shutdown requested");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn parse(bytes: &[u8]) -> Result<()> {
        let (sender, receiver) = mpsc::channel(MESSAGE.len());
        let bytes = bytes.to_vec();
        tokio::task::spawn_blocking(move || read(bytes.as_slice(), sender))
            .await
            .unwrap();
        request(receiver).await
    }

    #[tokio::test]
    async fn exact_stop_and_empty_parent_eof_are_the_only_valid_messages() {
        parse(b"stop\n").await.unwrap();
        parse(b"").await.unwrap();
        for bytes in [
            b"stop\r\n".as_slice(),
            b"STOP\n",
            b"stop ",
            b"s",
            b"stop",
            b"\n",
            &[0xff],
        ] {
            assert!(parse(bytes).await.is_err(), "{bytes:?}");
        }
    }

    #[tokio::test]
    async fn partial_command_deadline_and_read_failures_are_explicit() {
        let (sender, receiver) = mpsc::channel(MESSAGE.len());
        sender.send(Ok(b's')).await.unwrap();
        let error = request(receiver).await.unwrap_err();
        assert!(error.to_string().contains("deadline exceeded"));
        let (sender, receiver) = mpsc::channel(MESSAGE.len());
        sender
            .send(Err(std::io::Error::other("broken pipe fixture")))
            .await
            .unwrap();
        drop(sender);
        assert!(
            format!("{:#}", request(receiver).await.unwrap_err()).contains("broken pipe fixture")
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn stop_eof_and_invalid_command_all_drain_connected_characters_before_returning() {
        use crate::server::{
            account_actions,
            lifecycle_finalize_tests::{NativeSocket, socket_load, socket_server},
        };
        for message in [b"stop\n".as_slice(), b"", b"invalid\n"] {
            let (dir, app) = account_actions::tests::fixture();
            app.world.clock.set_paused(true);
            let account = app.store.list("desktop-drain".into()).await.unwrap().id;
            let character = app
                .store
                .create(account, app.content.get(crate::content::WIZARD).unwrap())
                .await
                .unwrap();
            let (port, server) = socket_server(&app).await;
            let mut socket = NativeSocket::connect(port, &app.origin).await;
            let player = socket_load(&mut socket, &dir, "desktop-drain", character.id).await;
            app.world
                .for_player(player)
                .call(move |world| {
                    world
                        .players
                        .get_mut(&player)
                        .unwrap()
                        .character
                        .lifetime
                        .shots += 7;
                    Ok(())
                })
                .await
                .unwrap();
            let result = super::super::drain_shutdown(&app, parse(message)).await;
            assert_eq!(result.is_ok(), message != b"invalid\n");
            assert!(app.owners.lock().unwrap().is_empty());
            let reopened = crate::store::Store::open(&dir.join("game.db"), &app.content).unwrap();
            let saved = reopened.load(account, character.id).await.unwrap();
            assert_eq!(saved.lifetime.shots, character.lifetime.shots + 7);
            assert!(saved.alive);
            drop(socket);
            server.abort();
        }
    }

    #[test]
    fn reader_is_bounded_even_without_a_newline() {
        let (sender, _receiver) = mpsc::channel(MESSAGE.len());
        let mut bytes = b"stopping forever".as_slice();
        read(&mut bytes, sender);
        assert_eq!(bytes, b"ing forever");
    }
}
