use std::time::Duration;

use clip_core::clipboard::{Clip, Clipboard, MemoryClipboard};
use clip_core::sync::Engine;
use clip_core::{pairing, State};
use tokio::net::{TcpListener, TcpStream};

async fn pair(
    host: &mut State,
    joiner: &mut State,
    host_code: &str,
    typed: &str,
) -> anyhow::Result<()> {
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
    engine_a
        .peers
        .lock()
        .unwrap()
        .insert(b.device.id.clone(), vec![addr_b]);
    let watch_a = engine_a.clone();
    tokio::spawn(async move { watch_a.watch().await });

    tokio::time::sleep(Duration::from_millis(100)).await;
    clip_a
        .set(&Clip::Text("hello from the mac".into()))
        .unwrap();

    let mut clip_b_read = clip_b.clone();
    for _ in 0..40 {
        if clip_b_read.get() == Some(Clip::Text("hello from the mac".into())) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("clip never arrived, b has {:?}", clip_b_read.get());
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
    assert!(engine_s
        .push(addr, &Clip::Text("sneaky".into()))
        .await
        .is_err());
    let mut clip_b = clip_b;
    assert_eq!(clip_b.get(), None);
}

/// Starts an engine listening on localhost; returns it with its address.
async fn serve(state: State, clip: MemoryClipboard) -> (Engine, std::net::SocketAddr) {
    let engine = Engine::new(state, None, Box::new(clip));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let e = engine.clone();
    tokio::spawn(async move { e.serve(listener).await });
    (engine, addr)
}

#[tokio::test]
async fn image_reaches_the_other_device() {
    let mut a = State::generate("mac").unwrap();
    let mut b = State::generate("windows").unwrap();
    pair(&mut a, &mut b, "222222", "222222").await.unwrap();

    let clip_b = MemoryClipboard::default();
    let (_engine_b, addr_b) = serve(b, clip_b.clone()).await;
    let engine_a = Engine::new(a, None, Box::new(MemoryClipboard::default()));

    let (width, height) = (64, 48);
    let rgba: Vec<u8> = (0..width * height * 4).map(|i| (i % 251) as u8).collect();
    let image = Clip::Image {
        width,
        height,
        rgba,
    };
    engine_a.push(addr_b, &image).await.unwrap();

    // push returns once sent; b applies it a moment later.
    let mut clip_b = clip_b;
    for _ in 0..40 {
        if clip_b.get().as_ref() == Some(&image) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("image never arrived");
}

#[tokio::test]
async fn removed_device_is_rejected_and_removal_spreads() {
    // a hosts the circle; b and c both join it.
    let mut a = State::generate("mac").unwrap();
    let mut b = State::generate("windows").unwrap();
    let mut c = State::generate("old-laptop").unwrap();
    pair(&mut a, &mut b, "333333", "333333").await.unwrap();
    pair(&mut a, &mut c, "444444", "444444").await.unwrap();
    // b learns about c the way it would in practice: via gossip from a.
    b.merge_members(&a.all_members());

    // a removes c, then syncs with b.
    a.remove_member("old-laptop").unwrap();
    let (engine_b, addr_b) = serve(b.clone(), MemoryClipboard::default()).await;
    let engine_a = Engine::new(a, None, Box::new(MemoryClipboard::default()));
    engine_a
        .push(addr_b, &Clip::Text("hi".into()))
        .await
        .unwrap();

    let b_now = engine_b.state().await;
    assert!(b_now.members.iter().all(|m| m.name != "old-laptop"));
    assert!(b_now.members.iter().any(|m| m.name == "mac"));

    // c can no longer push to b.
    let engine_c = Engine::new(c, None, Box::new(MemoryClipboard::default()));
    assert!(engine_c
        .push(addr_b, &Clip::Text("let me in".into()))
        .await
        .is_err());
}
