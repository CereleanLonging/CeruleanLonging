use super::*;

#[test]
fn parent_acl_accepts_attributes_and_rejects_listing_writes_or_inheritance() {
    use std::os::windows::ffi::OsStrExt;
    let directory = tempfile::tempdir().unwrap();
    let owner = crate::ipc::current_sid().unwrap();
    let worker = format!("{}-9999999", owner.rsplit_once('-').unwrap().0);
    let path: Vec<u16> = directory
        .path()
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    for (mask, flags, accepted) in [
        (0x20, "", true),
        (0x100020, "", true),
        (0xa0, "", true),
        (0x1000a0, "", true),
        (0x1000a1, "", false),
        (0x1000a2, "", false),
        (0x120089, "", false),
        (0x1f01ff, "", false),
        (0x1000a0, "OI", false),
        (0x1000a0, "CI", false),
    ] {
        let sddl: Vec<u16> = format!(
            "O:{owner}D:P(A;OICI;FA;;;{owner})(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;{flags};0x{mask:x};;;{worker})\0"
        )
        .encode_utf16()
        .collect();
        let mut descriptor = ptr::null_mut();
        assert_ne!(
            unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    sddl.as_ptr(),
                    1,
                    &mut descriptor,
                    ptr::null_mut(),
                )
            },
            0
        );
        let result = unsafe {
            SetFileSecurityW(
                path.as_ptr(),
                OWNER_SECURITY_INFORMATION
                    | DACL_SECURITY_INFORMATION
                    | PROTECTED_DACL_SECURITY_INFORMATION,
                descriptor,
            )
        };
        unsafe { LocalFree(descriptor) };
        assert_ne!(result, 0, "{}", std::io::Error::last_os_error());
        let checked = check_acl(directory.path(), &owner, None, true);
        assert_eq!(
            checked.is_ok(),
            accepted,
            "mask={mask:x}, flags={flags}, result={checked:?}"
        );
        assert!(check_acl(directory.path(), &owner, None, false).is_err());
    }
}
