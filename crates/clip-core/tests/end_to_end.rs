use std::time::Duration;

use clip_core::clipboard::{Clipboard, MemoryClipboard};
use clip_core::sync::Engine;
use clip_core::{pairing, State};
use tokio::net::{TcpListener, TcpStream};

async fn pair(host: &mut State, joiner: &mut State, host_code: &str, typed: &str) -> anyhow::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let code = host_code.to_owned();
    let mut h = host.clone();
    let host_task = tokio::spawn(async move {
        let r = pairing::host(&listener, &code, &mut h).await;
        (r, h)
    });
    let join = pairing::join(TcpStream::connect(addr).await?, typed, joiner).await;
    if join.is_err() {
        host_task.abort();
        return join;
    }
    let (r, h) = host_task.await?;
    r?;
    *host = h;
    Ok(())
}

#[tokio::test]
async fn pairing_with_right_code_joins_circle() {
    let mut a = State::generate("mac").unwrap();
    let mut b = State::generate("windows").unwrap();
    pair(&mut a, &mut b, "123456", "123456").await.unwrap();

    assert_eq!(a.circle_id, b.circle_id);
    assert_eq!(a.members, vec![b.device.clone()]);
    assert_eq!(b.members, vec![a.device.clone()]);
}

#[tokio::test]
async fn pairing_with_wrong_code_fails() {
    let mut a = State::generate("mac").unwrap();
    let mut b = State::generate("windows").unwrap();
    let before = b.circle_id.clone();
    assert!(pair(&mut a, &mut b, "123456", "654321").await.is_err());
    assert_eq!(b.circle_id, before);
    assert!(b.members.is_empty());
}

#[tokio::test]
async fn copy_on_one_device_pastes_on_the_other() {
    let mut a = State::generate("mac").unwrap();
    let mut b = State::generate("windows").unwrap();
    pair(&mut a, &mut b, "000042", "000042").await.unwrap();

    let mut clip_a = MemoryClipboard::default();
    let clip_b = MemoryClipboard::default();
    let engine_a = Engine::new(a.clone(), None, Box::new(clip_a.clone()));
    let engine_b = Engine::new(b.clone(), None, Box::new(clip_b.clone()));

    let listener_b = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr_b = listener_b.local_addr().unwrap();
    let serve_b = engine_b.clone();
    tokio::spawn(async move { serve_b.serve(listener_b).await });
    engine_a.peers.lock().unwrap().insert(b.device.id.clone(), vec![addr_b]);
    let watch_a = engine_a.clone();
    tokio::spawn(async move { watch_a.watch().await });

    tokio::time::sleep(Duration::from_millis(100)).await;
    clip_a.set_text("hello from the mac").unwrap();

    let mut clip_b_read = clip_b.clone();
    for _ in 0..40 {
        if clip_b_read.get_text().as_deref() == Some("hello from the mac") {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("clip never arrived, b has {:?}", clip_b_read.get_text());
}

#[tokio::test]
async fn device_outside_circle_is_rejected() {
    let mut a = State::generate("mac").unwrap();
    let mut b = State::generate("windows").unwrap();
    pair(&mut a, &mut b, "111111", "111111").await.unwrap();
    let stranger = State::generate("stranger").unwrap();

    let clip_b = MemoryClipboard::default();
    let engine_b = Engine::new(b, None, Box::new(clip_b.clone()));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let serve_b = engine_b.clone();
    tokio::spawn(async move { serve_b.serve(listener).await });

    let engine_s = Engine::new(stranger, None, Box::new(MemoryClipboard::default()));
    assert!(engine_s.push(addr, "sneaky").await.is_err());
    let mut clip_b = clip_b;
    assert_eq!(clip_b.get_text(), None);
}
