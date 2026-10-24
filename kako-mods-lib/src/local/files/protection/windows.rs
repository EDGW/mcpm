//! Windows ACL guards for content protection and directory-only removal access.

use std::{
    io,
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    ptr,
};

use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::{
        ACCESS_DENIED_ACE, ACE_HEADER, ACL,
        Authorization::{
            DENY_ACCESS, EXPLICIT_ACCESS_W, GetNamedSecurityInfoW, NO_MULTIPLE_TRUSTEE,
            SE_FILE_OBJECT, SetEntriesInAclW, SetNamedSecurityInfoW, TRUSTEE_IS_SID,
            TRUSTEE_IS_WELL_KNOWN_GROUP, TRUSTEE_W,
        },
        CreateWellKnownSid, DACL_SECURITY_INFORMATION, EqualSid, GetAce, INHERITED_ACE,
        NO_INHERITANCE, WinWorldSid,
    },
    Storage::FileSystem::{FILE_APPEND_DATA, FILE_DELETE_CHILD, FILE_WRITE_DATA},
    System::SystemServices::ACCESS_DENIED_ACE_TYPE,
};

pub(super) const CONTENT_WRITES: u32 = FILE_WRITE_DATA | FILE_APPEND_DATA;
pub(super) const DIRECTORY_WRITES: u32 = CONTENT_WRITES | FILE_DELETE_CHILD;

/// Keeps the original descriptor alive until the guard restores or commits its ACL.
pub(super) struct Protection {
    path: PathBuf,
    _descriptor: LocalAllocation,
    acl: *mut ACL,
}
impl Protection {
    fn open(path: &Path) -> io::Result<Self> {
        let wide = wide_path(path)?;
        let mut acl = ptr::null_mut();
        let mut descriptor = ptr::null_mut();
        // The returned descriptor owns acl for the lifetime of this guard.
        unsafe {
            check(GetNamedSecurityInfoW(
                wide.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                &mut acl,
                ptr::null_mut(),
                &mut descriptor,
            ))?;
        }
        let descriptor = LocalAllocation(descriptor);
        if acl.is_null() {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "content protection requires an existing non-null file DACL",
            ));
        }
        Ok(Self {
            path: path.to_owned(),
            _descriptor: descriptor,
            acl,
        })
    }

    fn set_acl(&self, acl: *const ACL) -> io::Result<()> {
        let wide = wide_path(&self.path)?;
        // acl is owned by a live descriptor or aligned buffer across this call.
        unsafe {
            check(SetNamedSecurityInfoW(
                wide.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                acl,
                ptr::null_mut(),
            ))
        }
    }

    pub(super) fn persist(mut self) {
        self.acl = ptr::null_mut();
    }
}
impl Drop for Protection {
    fn drop(&mut self) {
        if !self.acl.is_null() {
            let _ = self.set_acl(self.acl);
        }
    }
}

pub(super) fn deny(path: &Path, mask: u32) -> io::Result<Protection> {
    let protection = Protection::open(path)?;
    let mut sid = world_sid()?;
    let access = EXPLICIT_ACCESS_W {
        grfAccessPermissions: mask,
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
    let mut acl = ptr::null_mut();
    // The input descriptor and SID remain alive until the merged ACL is installed.
    unsafe {
        check(SetEntriesInAclW(1, &access, protection.acl, &mut acl))?;
    }
    let _allocation = LocalAllocation(acl.cast());
    protection.set_acl(acl)?;
    Ok(protection)
}

/// Clears only explicit Everyone FILE_DELETE_CHILD denials on this directory.
/// File ACLs, inherited ACEs and unrelated permissions are never changed.
pub(super) fn allow_removal(path: &Path) -> io::Result<Protection> {
    let protection = Protection::open(path)?;
    let mut sid = world_sid()?;
    // ACL.AclSize covers the header and all ACEs; u32 storage preserves alignment.
    let size = unsafe { (*protection.acl).AclSize as usize };
    let mut storage = vec![0_u32; size.div_ceil(std::mem::size_of::<u32>())];
    let acl = storage.as_mut_ptr().cast::<ACL>();
    // Copy before editing so the saved descriptor remains usable for rollback.
    unsafe {
        ptr::copy_nonoverlapping(protection.acl.cast::<u8>(), acl.cast::<u8>(), size);
        for index in 0..u32::from((*acl).AceCount) {
            let mut entry = ptr::null_mut();
            if GetAce(acl, index, &mut entry) == 0 {
                return Err(io::Error::last_os_error());
            }
            let header = &*entry.cast::<ACE_HEADER>();
            if u32::from(header.AceType) == ACCESS_DENIED_ACE_TYPE
                && u32::from(header.AceFlags) & INHERITED_ACE == 0
            {
                let denied = entry.cast::<ACCESS_DENIED_ACE>();
                if EqualSid(
                    ptr::addr_of_mut!((*denied).SidStart).cast(),
                    sid.as_mut_ptr().cast(),
                ) != 0
                {
                    (*denied).Mask &= !FILE_DELETE_CHILD;
                }
            }
        }
    }
    protection.set_acl(acl)?;
    Ok(protection)
}

fn wide_path(path: &Path) -> io::Result<Vec<u16>> {
    let mut wide: Vec<_> = path.as_os_str().encode_wide().collect();
    if wide.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path contains NUL",
        ));
    }
    wide.push(0);
    Ok(wide)
}

fn world_sid() -> io::Result<[u32; 17]> {
    let mut sid = [0_u32; 17];
    let mut size = std::mem::size_of_val(&sid) as u32;
    // SECURITY_MAX_SID_SIZE is 68 bytes; the stack buffer is aligned for a SID.
    if unsafe {
        CreateWellKnownSid(
            WinWorldSid,
            ptr::null_mut(),
            sid.as_mut_ptr().cast(),
            &mut size,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(sid)
}

fn check(code: u32) -> io::Result<()> {
    if code == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(code as i32))
    }
}

struct LocalAllocation(*mut std::ffi::c_void);
impl Drop for LocalAllocation {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}
