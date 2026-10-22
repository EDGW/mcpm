//! Opaque reply routing, request isolation and global-error decoding without business schemas.

use std::io::Cursor;

use serde_json::json;

use super::*;

#[test]
fn interleaved_opaque_replies_and_abandoned_receivers() {
    let shared = Shared {
        input: Mutex::new(Input { pipe: None }),
        routes: Mutex::new(Routes {
            closing: true,
            ..Default::default()
        }),
    };
    let (first_send, first) = mpsc::channel();
    let (second_send, second) = mpsc::channel();
    let a = Uuid::from_u128(1);
    let b = Uuid::from_u128(2);
    shared.routes.lock().unwrap().pending.insert(a, first_send);
    shared.routes.lock().unwrap().pending.insert(b, second_send);
    let frames = [
        json!({"guid":a,"status":"data","contents":{"whatever":42}}),
        json!({"guid":b,"status":"data","contents":["opaque",false]}),
        json!({"guid":b,"status":"finished","contents":null}),
        json!({"guid":a,"status":"finished","contents":null}),
        json!({"guid":Uuid::nil(),"status":"closed","contents":null}),
    ]
    .iter()
    .map(|value| format!("{value}\n"))
    .collect::<String>();
    drop(first);
    route(&mut Cursor::new(frames), &shared).unwrap();
    assert_eq!(
        second.recv().unwrap().unwrap().contents,
        json!(["opaque", false])
    );
    assert_eq!(second.recv().unwrap().unwrap().status, Status::Finished);
    assert!(shared.routes.lock().unwrap().closed);
    assert!(shared.routes.lock().unwrap().pending.is_empty());
}

#[cfg(unix)]
#[test]
fn parallel_handles_decode_global_errors_and_keep_contents_opaque() {
    use super::super::protocol::ErrorCode;
    let command = "test.command";
    let script = format!(
        r#"
printf '%s\n' '{{"guid":"{nil}","status":"ready","contents":{{"version":1,"commands":["{command}"],"max_concurrency":2}}}}'
IFS= read -r first
IFS= read -r second
first_guid=$(printf '%s' "$first" | cut -d '"' -f4)
second_guid=$(printf '%s' "$second" | cut -d '"' -f4)
printf '{{"guid":"%s","status":"data","contents":["opaque",false]}}\n{{"guid":"%s","status":"finished","contents":null}}\n' "$second_guid" "$second_guid"
printf '{{"guid":"%s","status":"finished","contents":null}}\n' "$first_guid"
IFS= read -r third
IFS= read -r cancel
third_guid=$(printf '%s' "$third" | cut -d '"' -f4)
printf '{{"guid":"%s","status":"error","contents":{{"code":"cancelled","message":"cancelled"}}}}\n' "$third_guid"
cat >/dev/null
printf '%s\n' '{{"guid":"{nil}","status":"closed","contents":null}}'
"#,
        nil = Uuid::nil()
    );
    let mut session = Session::start(ProcessCommand::new("sh").args(["-c", &script])).unwrap();
    let first = session.submit(command, json!({"input":1})).unwrap();
    let second = session.submit(command, json!({"input":2})).unwrap();
    assert!(!first.guid.is_nil());
    assert!(!second.guid.is_nil());
    assert_ne!(first.guid, second.guid);
    let mut contents = Vec::new();
    second
        .collect(|reply| {
            contents.push(reply.decode::<serde_json::Value>()?);
            Ok(())
        })
        .unwrap();
    assert_eq!(contents, [json!(["opaque", false])]);
    first
        .collect(|_| panic!("first request has no data"))
        .unwrap();
    let third = session.submit(command, json!({})).unwrap();
    third.cancel().unwrap();
    let error = third
        .collect(|_| panic!("global error is not command contents"))
        .unwrap_err();
    assert_eq!(
        error.downcast_ref::<GlobalError>().unwrap().code,
        ErrorCode::Cancelled
    );
    session.close().unwrap();
}
