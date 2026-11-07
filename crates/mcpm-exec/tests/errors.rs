//! Consumer-visible preservation of aggregate failures across operation sessions.

use std::sync::Arc;

use anyhow::Result;
use mcpm_exec::{
    error::{self, CombinedError, ErrorReport, PortableError, RemoteError},
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

struct PartialFailure;
impl Operation for PartialFailure {
    const ID: &'static str = "test.partial_failure";
    type Input = ();
    type Output = Vec<Result<u32, PortableError>>;

    async fn run(_: ()) -> Result<Self::Output> {
        Ok(vec![Ok(7), Err(failure().into()), Ok(9)])
    }
}

fn session() -> Session {
    let (client, server) = tokio::io::duplex(128);
    let mut server = Session::new(server);
    let mut registry = Registry::new();
    registry.register::<Fail>().unwrap();
    registry.register::<FailStream>().unwrap();
    registry.register::<PartialFailure>().unwrap();
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

#[tokio::test]
async fn portable_item_failure_preserves_local_identity_and_remote_tree() {
    let mut local = operation::execute::<PartialFailure>(())
        .await
        .unwrap()
        .into_iter();
    assert_eq!(local.next().unwrap().unwrap(), 7);
    let local_error = local.next().unwrap().unwrap_err().into_inner();
    assert!(!error::contains::<RemoteError>(local_error.as_ref()));
    assert!(local_error.downcast_ref::<CombinedError>().is_some());
    assert_eq!(
        ErrorReport::capture(&local_error),
        ErrorReport::capture(&failure())
    );
    assert_eq!(local.next().unwrap().unwrap(), 9);
    assert!(local.next().is_none());

    let client = session();
    let mut remote = operation::call::<PartialFailure>(&client, &())
        .await
        .unwrap()
        .into_iter();
    assert_eq!(remote.next().unwrap().unwrap(), 7);
    let remote_error = remote.next().unwrap().unwrap_err().into_inner();
    assert!(error::contains::<RemoteError>(remote_error.as_ref()));
    assert!(error::contains::<CombinedError>(remote_error.as_ref()));
    assert_eq!(
        ErrorReport::capture(&remote_error),
        ErrorReport::capture(&failure())
    );
    assert_eq!(remote.next().unwrap().unwrap(), 9);
    assert!(remote.next().is_none());

    // A second transport hop must not accumulate remote markers or lose branches.
    let bytes = serde_json::to_vec(&PortableError::from(remote_error)).unwrap();
    let forwarded: PortableError = serde_json::from_slice(&bytes).unwrap();
    let forwarded = forwarded.into_inner();
    assert_eq!(
        forwarded
            .chain()
            .filter(|cause| cause.is::<RemoteError>())
            .count(),
        1
    );
    assert_eq!(
        ErrorReport::capture(&forwarded),
        ErrorReport::capture(&failure())
    );
}
