//! Per-user named pipes protected by a current-user-only discretionary ACL.

use super::AlreadyRunning;
use anyhow::{Context, Result, ensure};
use std::{ffi::c_void, io, path::PathBuf, ptr};
use tokio::{
    io::{ReadHalf, WriteHalf},
    net::windows::named_pipe::{ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions},
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, LocalFree},
    Security::{
        Authorization::{
            ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
        },
        GetTokenInformation, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser,
    },
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};

#[derive(Clone, Debug)]
pub struct Endpoint {
    name: String,
    key: String,
    directory: PathBuf,
    sid: String,
}

impl Endpoint {
    pub fn for_key(key: &str) -> Result<Self> {
        super::validate_key(key)?;
        let sid = current_sid()?;
        let user = blake3::hash(sid.as_bytes()).to_hex()[..8].to_owned();
        let directory =
            PathBuf::from(std::env::var_os("LOCALAPPDATA").context("LOCALAPPDATA is unavailable")?)
                .join("mcpm/link");
        std::fs::create_dir_all(&directory).context("failed to create link state directory")?;
        Ok(Self {
            name: format!(r"\\.\pipe\mcpm-link-{user}-{key}"),
            key: key.to_owned(),
            directory,
            sid,
        })
    }

    pub fn directories() -> Vec<PathBuf> {
        std::env::var_os("LOCALAPPDATA")
            .map(|path| vec![PathBuf::from(path).join("mcpm/link")])
            .unwrap_or_default()
    }
    pub fn state_path(&self) -> PathBuf {
        self.directory.join(format!("{}.json", self.key))
    }
    pub fn log_path(&self) -> PathBuf {
        self.directory.join(format!("{}.log", self.key))
    }
    pub fn key(&self) -> &str {
        &self.key
    }

    pub async fn connect(&self) -> Result<(ReadHalf<NamedPipeClient>, WriteHalf<NamedPipeClient>)> {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            match ClientOptions::new().open(&self.name) {
                Ok(pipe) => return Ok(tokio::io::split(pipe)),
                Err(error)
                    if error.raw_os_error() == Some(231)
                        && tokio::time::Instant::now() < deadline =>
                {
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
                Err(error) => return Err(error).context("failed to connect to link pipe"),
            }
        }
    }

    pub fn listen(&self) -> Result<Listener> {
        let pipe = self.create(true).map_err(|error| {
            if error
                .downcast_ref::<io::Error>()
                .is_some_and(|error| error.raw_os_error() == Some(5))
            {
                anyhow::Error::new(AlreadyRunning)
            } else {
                error
            }
        })?;
        Ok(Listener {
            endpoint: self.clone(),
            pipe,
        })
    }

    fn create(&self, first: bool) -> Result<NamedPipeServer> {
        let sddl: Vec<u16> = format!("D:P(A;;GA;;;{})", self.sid)
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let mut descriptor = ptr::null_mut();
        ensure!(
            unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    sddl.as_ptr(),
                    1,
                    &mut descriptor,
                    ptr::null_mut(),
                )
            } != 0,
            "failed to create link pipe security descriptor: {}",
            io::Error::last_os_error()
        );
        let mut attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        let result = unsafe {
            ServerOptions::new()
                .first_pipe_instance(first)
                .reject_remote_clients(true)
                .create_with_security_attributes_raw(
                    &self.name,
                    (&mut attributes as *mut SECURITY_ATTRIBUTES).cast::<c_void>(),
                )
        };
        unsafe { LocalFree(descriptor) };
        Ok(result?)
    }
}

pub struct Listener {
    endpoint: Endpoint,
    pipe: NamedPipeServer,
}

impl Listener {
    pub async fn accept(
        &mut self,
    ) -> Result<(ReadHalf<NamedPipeServer>, WriteHalf<NamedPipeServer>)> {
        self.pipe
            .connect()
            .await
            .context("failed to accept link pipe client")?;
        let next = self.endpoint.create(false)?;
        let connected = std::mem::replace(&mut self.pipe, next);
        Ok(tokio::io::split(connected))
    }
}

fn current_sid() -> Result<String> {
    let mut token = ptr::null_mut();
    ensure!(
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } != 0,
        "failed to open process token: {}",
        io::Error::last_os_error()
    );
    let result = (|| {
        let mut length = 0;
        unsafe { GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut length) };
        ensure!(
            length > 0,
            "failed to query token user: {}",
            io::Error::last_os_error()
        );
        let mut buffer = vec![0usize; (length as usize).div_ceil(std::mem::size_of::<usize>())];
        ensure!(
            unsafe {
                GetTokenInformation(
                    token,
                    TokenUser,
                    buffer.as_mut_ptr().cast(),
                    length,
                    &mut length,
                )
            } != 0,
            "failed to read token user: {}",
            io::Error::last_os_error()
        );
        let user = unsafe { &*(buffer.as_ptr().cast::<TOKEN_USER>()) };
        let mut sid = ptr::null_mut();
        ensure!(
            unsafe { ConvertSidToStringSidW(user.User.Sid, &mut sid) } != 0,
            "failed to format user SID: {}",
            io::Error::last_os_error()
        );
        let mut count = 0;
        unsafe {
            while *sid.add(count) != 0 {
                count += 1;
            }
        }
        let result = String::from_utf16(unsafe { std::slice::from_raw_parts(sid, count) })
            .context("user SID is not UTF-16");
        unsafe { LocalFree(sid.cast()) };
        result
    })();
    unsafe { CloseHandle(token) };
    result
}
