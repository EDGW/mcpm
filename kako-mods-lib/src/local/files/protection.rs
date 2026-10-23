//! Platform-specific content write protection that preserves unlink and replacement.

use std::{io, path::Path};

/// Protects file contents without making the parent directory immutable.
///
/// Unix removes all write bits. Windows adds a deny ACE for Everyone covering
/// FILE_WRITE_DATA and FILE_APPEND_DATA, preserving other access rights and the
/// delete behavior of the file/parent directory. No read-only attribute is set.
/// Owners/administrators can still change permissions; this prevents ordinary
/// in-place writes rather than providing an adversarial immutability boundary.
pub fn protect_file_contents(path: &Path) -> io::Result<()> {
    protect(path)
}

#[cfg(unix)]
fn protect(path: &Path) -> io::Result<()> {
    use std::{fs, os::unix::fs::PermissionsExt};
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(permissions.mode() & !0o222);
    fs::set_permissions(path, permissions)
}

#[cfg(windows)]
fn protect(path: &Path) -> io::Result<()> {
    use std::{os::windows::ffi::OsStrExt, ptr};
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::{
            Authorization::{
                DENY_ACCESS, EXPLICIT_ACCESS_W, GetNamedSecurityInfoW, NO_MULTIPLE_TRUSTEE,
                SE_FILE_OBJECT, SetEntriesInAclW, SetNamedSecurityInfoW, TRUSTEE_IS_SID,
                TRUSTEE_IS_WELL_KNOWN_GROUP, TRUSTEE_W,
            },
            CreateWellKnownSid, DACL_SECURITY_INFORMATION, NO_INHERITANCE, WinWorldSid,
        },
        Storage::FileSystem::{FILE_APPEND_DATA, FILE_WRITE_DATA},
    };

    // The APIs allocate descriptors/ACLs with LocalAlloc; release on every exit.
    struct LocalAllocation(*mut std::ffi::c_void);
    impl Drop for LocalAllocation {
        fn drop(&mut self) {
            unsafe {
                LocalFree(self.0);
            }
        }
    }
    fn check(code: u32) -> io::Result<()> {
        if code == 0 {
            Ok(())
        } else {
            Err(io::Error::from_raw_os_error(code as i32))
        }
    }

    let wide: Vec<_> = path.as_os_str().encode_wide().collect();
    if wide.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path contains NUL",
        ));
    }
    let wide: Vec<_> = wide.into_iter().chain(Some(0)).collect();
    let mut old_acl = ptr::null_mut();
    let mut descriptor = ptr::null_mut();
    // SAFETY: paths are terminated UTF-16; out pointers remain valid. The
    // descriptor owns old_acl until after the new ACL has been constructed.
    unsafe {
        check(GetNamedSecurityInfoW(
            wide.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut old_acl,
            ptr::null_mut(),
            &mut descriptor,
        ))?;
        let _descriptor = LocalAllocation(descriptor);
        if old_acl.is_null() {
            // NULL DACL means unrestricted access. Building a deny-only DACL
            // would also remove read/delete access, violating this API's contract.
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "content protection requires an existing non-null file DACL",
            ));
        }
        // SECURITY_MAX_SID_SIZE is 68 bytes; u32 storage keeps SID fields aligned.
        let mut sid = [0u32; 17];
        let mut sid_size = std::mem::size_of_val(&sid) as u32;
        if CreateWellKnownSid(
            WinWorldSid,
            ptr::null_mut(),
            sid.as_mut_ptr().cast(),
            &mut sid_size,
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        let access = EXPLICIT_ACCESS_W {
            grfAccessPermissions: FILE_WRITE_DATA | FILE_APPEND_DATA,
            grfAccessMode: DENY_ACCESS,
            grfInheritance: NO_INHERITANCE,
            Trustee: TRUSTEE_W {
                pMultipleTrustee: ptr::null_mut(),
                MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
                TrusteeForm: TRUSTEE_IS_SID,
                TrusteeType: TRUSTEE_IS_WELL_KNOWN_GROUP,
                ptstrName: sid.as_mut_ptr().cast(),
            },
        };
        let mut new_acl = ptr::null_mut();
        check(SetEntriesInAclW(1, &access, old_acl, &mut new_acl))?;
        let _new_acl = LocalAllocation(new_acl.cast());
        check(SetNamedSecurityInfoW(
            wide.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            new_acl,
            ptr::null_mut(),
        ))
    }
}

#[cfg(not(any(unix, windows)))]
fn protect(_path: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "content write protection is unsupported on this platform",
    ))
}
