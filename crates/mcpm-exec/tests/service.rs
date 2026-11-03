//! Execution and relay invariants that differ between local calls and transported calls.

use std::time::Duration;

use mcpm_exec::{
    messaging::{ChannelError, Session, relay},
    operation::Operation,
    service::{Executor, Target},
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::json;

struct LocalInput(u64);

impl Serialize for LocalInput {
    fn serialize<S: Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
        panic!("local execution must not serialize its input")
    }
}

impl<'de> Deserialize<'de> for LocalInput {
    fn deserialize<D: Deserializer<'de>>(_: D) -> Result<Self, D::Error> {
        panic!("local execution must not deserialize its input")
    }
}

struct Increment;

impl Operation for Increment {
    const ID: &'static str = "test.increment";
    type Input = LocalInput;
    type Output = u64;

    async fn run(input: LocalInput) -> anyhow::Result<u64> {
        Ok(input.0 + 1)
    }
}

#[tokio::test]
async fn local_execution_preserves_typed_input_without_serde() {
    let executor = Executor::open(Target::Local).await.unwrap();
    assert_eq!(
        executor.call::<Increment>(LocalInput(41)).await.unwrap(),
        42
    );
}

#[tokio::test]
async fn relay_preserves_order_and_final_body_in_both_directions() {
    tokio::time::timeout(Duration::from_secs(5), async {
        for reverse in [false, true] {
            let (client_io, link_io) = tokio::io::duplex(128);
            let client = Session::new(client_io);
            let mut incoming = Session::new(link_io);
            let (link_io, server_io) = tokio::io::duplex(128);
            let outgoing = Session::new(link_io);
            let mut server = Session::new(server_io);
            let mut caller = client.open();
            caller.send(json!("request")).unwrap();
            let inbound = incoming.accept().await.unwrap().unwrap();
            let task = tokio::spawn(async move {
                let _ = relay(inbound, outgoing.open()).await;
            });
            let mut callee = server.accept().await.unwrap().unwrap();
            assert_eq!(callee.recv().await.unwrap(), Some(json!("request")));
            if reverse {
                callee.send(json!({"sequence": 1})).unwrap();
                callee.finish(Some(json!({"sequence": 2}))).unwrap();
                assert_eq!(caller.recv().await.unwrap(), Some(json!({"sequence": 1})));
                assert_eq!(caller.recv().await.unwrap(), Some(json!({"sequence": 2})));
                assert_eq!(caller.recv().await.unwrap(), None);
            } else {
                caller.send(json!({"sequence": 1})).unwrap();
                caller.finish(Some(json!({"sequence": 2}))).unwrap();
                assert_eq!(callee.recv().await.unwrap(), Some(json!({"sequence": 1})));
                assert_eq!(callee.recv().await.unwrap(), Some(json!({"sequence": 2})));
                assert_eq!(callee.recv().await.unwrap(), None);
            }
            task.await.unwrap();
        }
    })
    .await
    .expect("relay did not finish");
}

#[tokio::test]
async fn relay_aborts_remote_channel_when_client_disconnects() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let (client_io, link_io) = tokio::io::duplex(128);
        let client = Session::new(client_io);
        let mut incoming = Session::new(link_io);
        let (link_io, server_io) = tokio::io::duplex(128);
        let outgoing = Session::new(link_io);
        let mut server = Session::new(server_io);
        let caller = client.open();
        caller.send(json!("start")).unwrap();
        let inbound = incoming.accept().await.unwrap().unwrap();
        let task = tokio::spawn(async move {
            let _ = relay(inbound, outgoing.open()).await;
        });
        let mut callee = server.accept().await.unwrap().unwrap();
        assert_eq!(callee.recv().await.unwrap(), Some(json!("start")));
        drop(client);
        drop(caller);
        assert!(matches!(callee.recv().await, Err(ChannelError::Aborted(_))));
        task.await.unwrap();
    })
    .await
    .expect("client disconnect did not cancel the forwarded call");
}
