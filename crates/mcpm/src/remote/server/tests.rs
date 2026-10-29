//! Concurrent opaque routing, overload, cancellation and EOF drain without command codec assumptions.

use std::io::{BufReader, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::time::Duration;

use serde_json::{Value, json};

use super::*;
use crate::remote::command::Command;

#[derive(Debug)]
struct Blocking;
impl Command for Blocking {
    const ID: &'static str = "test.blocking";
    fn run(contents: Value, output: &Output) -> Result<()> {
        output.send(contents.clone())?;
        if contents == json!(true) {
            loop {
                output.checkpoint()?;
                thread::sleep(Duration::from_millis(1));
            }
        }
        Ok(())
    }
}
fn send(stream: &mut TcpStream, guid: u128, contents: Value) {
    writeln!(
        stream,
        "{}",
        json!({"guid":Uuid::from_u128(guid),"command":Blocking::ID,"contents":contents})
    )
    .unwrap();
    stream.flush().unwrap();
}
fn event(reader: &mut BufReader<TcpStream>) -> Value {
    protocol::read(reader).unwrap().unwrap()
}
#[test]
fn concurrent_dispatch_cancellation_overload_and_eof_drain() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let (server, _) = listener.accept().unwrap();
    let mut registry = Registry::default();
    registry.register::<Blocking>().unwrap();
    let worker = thread::spawn(move || {
        serve(
            BufReader::new(server.try_clone().unwrap()),
            server,
            registry,
            2,
        )
    });
    let mut reader = BufReader::new(client.try_clone().unwrap());
    assert_eq!(
        event(&mut reader)["contents"]["commands"],
        json!([Blocking::ID])
    );
    send(&mut client, 1, json!(true));
    assert_eq!(event(&mut reader)["contents"], true);
    let opaque = json!({"not_a_list":[1,"raw",{"nested":false}]});
    send(&mut client, 2, opaque.clone());
    assert_eq!(event(&mut reader)["contents"], opaque);
    assert_eq!(
        event(&mut reader),
        json!({"guid":Uuid::from_u128(2),"status":"finished","contents":null})
    );
    send(&mut client, 3, json!(true));
    assert_eq!(event(&mut reader)["contents"], true);
    send(&mut client, 4, json!(false));
    assert_eq!(event(&mut reader)["contents"]["code"], "busy");
    for guid in [1, 3] {
        writeln!(client, "{}", json!({"cancel":Uuid::from_u128(guid)})).unwrap();
        let reply = event(&mut reader);
        assert_eq!(reply["guid"], json!(Uuid::from_u128(guid)));
        assert_eq!(reply["status"], "error");
        assert_eq!(reply["contents"]["code"], "cancelled");
    }
    send(&mut client, 5, json!(false));
    client.shutdown(Shutdown::Write).unwrap();
    assert_eq!(event(&mut reader)["contents"], false);
    assert_eq!(event(&mut reader)["status"], "finished");
    assert_eq!(
        event(&mut reader),
        json!({"guid":Uuid::nil(),"status":"closed","contents":null})
    );
    worker.join().unwrap().unwrap();
}
