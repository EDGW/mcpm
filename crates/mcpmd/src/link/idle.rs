//! Idle expiry depends on both client and operation leases, never on traffic volume.

use super::control::State;
use std::time::Duration;

pub async fn expired(state: &State, seconds: u64) {
    match (seconds, state.idle_since()) {
        (0, _) | (_, None) => std::future::pending::<()>().await,
        (_, Some(started)) => {
            tokio::time::sleep_until((started + Duration::from_secs(seconds)).into()).await
        }
    }
}
