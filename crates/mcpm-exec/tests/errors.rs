//! Consumer-visible preservation of aggregate failures across operation sessions.

use std::sync::Arc;

use anyhow::Result;
use mcpm_exec::{
    error::{self, CombinedError, ErrorReport, RemoteError},
    messaging::Session,
    operation::{self, Operation, Registry, StreamingOutput},
};

fn failure() -> anyhow::Error {
    anyhow::Error::new(CombinedError::new(
        anyhow::anyhow!("publication failed").context("publish snapshot"),
        anyhow::Error::new(CombinedError::new(
            anyhow::anyhow!("cleanup failed"),
            anyhow::anyhow!("rollback failed"),
        ))
        .context("recover snapshot"),
    ))
    .context("save snapshot")
}

struct Fail;
impl Operation for Fail {
    const ID: &'static str = "test.fail";
    type Input = bool;
    type Output = ();

    async fn run(aggregate: bool) -> Result<()> {
        Err(if aggregate {
            failure()
        } else {
            anyhow::anyhow!("missing file").context("read snapshot")
        })
    }
}

struct FailStream;
impl Operation for FailStream {
    const ID: &'static str = "test.fail_stream";
    type Input = ();
    type Output = StreamingOutput<u32>;

    async fn run(_: ()) -> Result<Self::Output> {
        Ok(StreamingOutput::new(|items| async move {
            items.send(7).await?;
            Err(failure())
        }))
    }
}

fn session() -> Session {
    let (client, server) = tokio::io::duplex(128);
    let mut server = Session::new(server);
    let mut registry = Registry::new();
    registry.register::<Fail>().unwrap();
    registry.register::<FailStream>().unwrap();
    tokio::spawn(async move {
        operation::serve(&mut server, Arc::new(registry))
            .await
            .unwrap();
    });
    Session::new(client)
}

fn assert_aggregate(error: &anyhow::Error) {
    assert!(error::contains::<RemoteError>(error.as_ref()));
    assert!(error::contains::<CombinedError>(error.as_ref()));
    let report = ErrorReport::capture(error);
    assert_eq!(report.causes, ["save snapshot"]);
    assert_eq!(report.branches, ErrorReport::capture(&failure()).branches);
    assert_eq!(report.branches[0].message, "publish snapshot");
    assert_eq!(report.branches[0].causes, ["publication failed"]);
    assert_eq!(report.branches[1].message, "recover snapshot");
    assert_eq!(report.branches[1].branches[0].message, "cleanup failed");
    assert_eq!(report.branches[1].branches[1].message, "rollback failed");
    // Forwarding a received failure must not flatten branches or duplicate remote wrappers.
    let restored = RemoteError::wrap(report.clone().into_error());
    assert_eq!(ErrorReport::capture(&restored), report);
}

#[tokio::test]
async fn operation_failures_preserve_nested_branches_and_linear_causes() {
    let client = session();
    let error = operation::call::<Fail>(&client, &true).await.unwrap_err();
    assert_aggregate(&error);

    let linear = operation::call::<Fail>(&client, &false).await.unwrap_err();
    assert!(error::contains::<RemoteError>(linear.as_ref()));
    let report = ErrorReport::capture(&linear);
    assert_eq!(report.causes, ["read snapshot", "missing file"]);
    assert!(report.branches.is_empty());

    let local = operation::execute::<Fail>(true).await.unwrap_err();
    assert!(!error::contains::<RemoteError>(local.as_ref()));
    assert_eq!(
        ErrorReport::capture(&local),
        ErrorReport::capture(&failure())
    );
}

#[tokio::test]
async fn stream_delivers_prior_items_then_the_complete_failure_tree() {
    let client = session();
    let mut stream = operation::call::<FailStream>(&client, &()).await.unwrap();
    assert_eq!(stream.next().await.unwrap().unwrap(), 7);
    let error = stream.next().await.unwrap().unwrap_err();
    assert_aggregate(&error);
    assert!(stream.next().await.is_none());
}
