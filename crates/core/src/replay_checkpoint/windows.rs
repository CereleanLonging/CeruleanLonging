use super::{MAX_PLAIN, MAX_SEALED};
use std::ptr;
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    },
};
use zeroize::{Zeroize, Zeroizing};
struct Allocation(CRYPT_INTEGER_BLOB);
impl Drop for Allocation {
    fn drop(&mut self) {
        unsafe {
            if !self.0.pbData.is_null() {
                std::slice::from_raw_parts_mut(self.0.pbData, self.0.cbData as usize).zeroize();
                LocalFree(self.0.pbData.cast());
            }
        }
    }
}
pub(super) fn crypt(
    bytes: &[u8],
    entropy: &[u8],
    decrypt: bool,
) -> Result<Zeroizing<Vec<u8>>, String> {
    if bytes.len() > if decrypt { MAX_SEALED } else { MAX_PLAIN } {
        return Err("checkpoint DPAPI input limit".into());
    }
    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_ptr().cast_mut(),
    };
    let extra = CRYPT_INTEGER_BLOB {
        cbData: entropy
            .len()
            .try_into()
            .map_err(|_| "checkpoint entropy limit")?,
        pbData: entropy.as_ptr().cast_mut(),
    };
    let mut output = Allocation(CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: ptr::null_mut(),
    });
    let ok = unsafe {
        if decrypt {
            CryptUnprotectData(
                &input,
                ptr::null_mut(),
                &extra,
                ptr::null(),
                ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output.0,
            )
        } else {
            CryptProtectData(
                &input,
                ptr::null(),
                &extra,
                ptr::null(),
                ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output.0,
            )
        }
    };
    if ok == 0
        || output.0.pbData.is_null()
        || output.0.cbData as usize > if decrypt { MAX_PLAIN } else { MAX_SEALED }
    {
        return Err("checkpoint current-user authentication failed".into());
    }
    Ok(Zeroizing::new(
        unsafe { std::slice::from_raw_parts(output.0.pbData, output.0.cbData as usize) }.to_vec(),
    ))
}
