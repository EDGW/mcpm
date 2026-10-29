//! Server metadata acquisition and SSH command registration.

mod remote;
pub(super) use crate::server::ServerInfo;
pub(super) use remote::load as load_ssh;

pub(super) fn remote_view(remote: &super::workspace::Remote) -> crate::server::RemoteInfo {
    let connection = remote.connection();
    crate::server::RemoteInfo {
        path: remote.path.clone(),
        ssh: connection.ssh.clone(),
        destination: connection.destination(),
        executable: connection.executable().into(),
    }
}
