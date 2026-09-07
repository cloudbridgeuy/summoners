#![allow(clippy::expect_used)]

use super::*;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU8, Ordering},
};

#[derive(Clone)]
struct FailingWriter {
    bytes: Arc<Mutex<Vec<u8>>>,
    mode: Arc<AtomicU8>,
}
impl Write for FailingWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if self.mode.load(Ordering::SeqCst) == 1 {
            return Err(io::Error::other("injected write failure"));
        }
        self.bytes
            .lock()
            .expect("bytes lock")
            .extend_from_slice(buffer);
        Ok(buffer.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        if self.mode.load(Ordering::SeqCst) == 2 {
            return Err(io::Error::other("injected flush failure"));
        }
        Ok(())
    }
}
#[tokio::test]
async fn connection_queue_holds_sixteen_messages() {
    let (sender, _receiver) = mpsc::channel::<Outbound>(CONNECTION_QUEUE);
    for _ in 0..CONNECTION_QUEUE {
        let (completed, _received) = oneshot::channel();
        sender
            .try_send(Outbound {
                envelope: ServerEnvelope::Stopped {
                    reason: "test".to_string(),
                },
                completed,
            })
            .expect("queue accepts capacity");
    }
    let (completed, _received) = oneshot::channel();
    assert!(matches!(
        sender.try_send(Outbound {
            envelope: ServerEnvelope::Stopped {
                reason: "test".to_string()
            },
            completed
        }),
        Err(mpsc::error::TrySendError::Full(_))
    ));
}
async fn connect_seats(address: std::net::SocketAddr) -> (TcpStream, TcpStream) {
    let mut one = TcpStream::connect(address).await.expect("one connects");
    let join = serde_json::to_vec(&ClientEnvelope::Join {
        version: VERSION,
        seat: Seat::One,
    })
    .expect("one join");
    one.write_all(&[join, vec![b'\n']].concat())
        .await
        .expect("one writes");
    assert!(matches!(
        serde_json::from_slice::<ServerEnvelope>(&read_frame(&mut one).await.expect("one waiting"))
            .expect("one envelope"),
        ServerEnvelope::Waiting { .. }
    ));
    let mut two = TcpStream::connect(address).await.expect("two connects");
    let join = serde_json::to_vec(&ClientEnvelope::Join {
        version: VERSION,
        seat: Seat::Two,
    })
    .expect("two join");
    two.write_all(&[join, vec![b'\n']].concat())
        .await
        .expect("two writes");
    assert!(matches!(
        serde_json::from_slice::<ServerEnvelope>(&read_frame(&mut two).await.expect("two waiting"))
            .expect("two envelope"),
        ServerEnvelope::Waiting { .. }
    ));
    for stream in [&mut one, &mut two] {
        assert!(matches!(
            serde_json::from_slice::<ServerEnvelope>(&read_frame(stream).await.expect("opening"))
                .expect("opening envelope"),
            ServerEnvelope::Update { .. }
        ));
    }
    (one, two)
}
async fn recorder_failure_stops_seats(mode: u8) {
    let catalog = built_in_catalog().expect("catalog");
    let state = initial_state(
        catalog.library(),
        [catalog.set_paths(), catalog.barrow_herd()],
        1,
    )
    .expect("state");
    let descriptions = crate::protocol::CardDescriptions::from_card_set(&state.cards);
    let identities = crate::protocol::CardIdentityMap::from_initial_state(&state, &descriptions);
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let failure = Arc::new(AtomicU8::new(0));
    let recorder = start_recording(
        FailingWriter {
            bytes: Arc::clone(&bytes),
            mode: Arc::clone(&failure),
        },
        BTreeMap::new(),
        required_sets(catalog.library()).expect("sets"),
        state,
    )
    .expect("recorder");
    failure.store(mode, Ordering::SeqCst);
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
    let address = listener.local_addr().expect("address");
    let clients = async {
        let (mut one, mut two) = connect_seats(address).await;
        let action = serde_json::to_vec(&json!({"kind":"submit","version":1,"request_id":1,"based_on_revision":0,"action":{"kind":"end_turn","player":"one"}})).expect("action");
        one.write_all(&[action, vec![b'\n']].concat())
            .await
            .expect("action writes");
        for stream in [&mut one, &mut two] {
            assert!(
                matches!(serde_json::from_slice::<ServerEnvelope>(&read_frame(stream).await.expect("stopped")).expect("stopped envelope"), ServerEnvelope::Stopped { reason } if reason == "recording failed")
            );
        }
    };
    let (result, ()) = tokio::join!(
        serve_session(listener, recorder, &descriptions, &identities),
        clients
    );
    assert!(matches!(result, Err(ServeError::Recording(_))));
    assert!(
        !String::from_utf8(bytes.lock().expect("bytes lock").clone())
            .expect("transcript text")
            .contains("\"record\":\"match_completed\"")
    );
}
#[tokio::test]
async fn recorder_write_failure_before_engine_application_stops_both_seats() {
    recorder_failure_stops_seats(1).await;
}
#[tokio::test]
async fn recorder_flush_failure_after_engine_application_stops_both_seats() {
    recorder_failure_stops_seats(2).await;
}
