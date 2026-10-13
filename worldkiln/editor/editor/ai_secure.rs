// Worldkiln AI | WO 0.8.7-65 | 2026-10-09
// Windows DPAPI CurrentUser-protected credentials, separate from chat/preferences.
use super::ai_provider::Provider;
use std::{collections::HashMap, ffi::c_void, fs, path::Path};

#[cfg(windows)]
mod dpapi {
    use super::*;
    #[repr(C)]
    struct DataBlob {
        size: u32,
        bytes: *mut u8,
    }
    #[link(name = "crypt32")]
    unsafe extern "system" {
        fn CryptProtectData(
            input: *mut DataBlob,
            description: *const u16,
            entropy: *mut DataBlob,
            reserved: *mut c_void,
            prompt: *mut c_void,
            flags: u32,
            output: *mut DataBlob,
        ) -> i32;
        fn CryptUnprotectData(
            input: *mut DataBlob,
            description: *mut *mut u16,
            entropy: *mut DataBlob,
            reserved: *mut c_void,
            prompt: *mut c_void,
            flags: u32,
            output: *mut DataBlob,
        ) -> i32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LocalFree(p: *mut c_void) -> *mut c_void;
    }
    fn transform(source: &[u8], encrypt: bool) -> Result<Vec<u8>, String> {
        let len: u32 = source
            .len()
            .try_into()
            .map_err(|_| "Credentials too large.".to_owned())?;
        let mut input = DataBlob {
            size: len,
            bytes: source.as_ptr() as *mut u8,
        };
        let mut output = DataBlob {
            size: 0,
            bytes: std::ptr::null_mut(),
        };
        let success = unsafe {
            if encrypt {
                CryptProtectData(
                    &mut input,
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    1,
                    &mut output,
                )
            } else {
                CryptUnprotectData(
                    &mut input,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    1,
                    &mut output,
                )
            }
        };
        if success == 0 {
            return Err(format!(
                "Windows credential protection failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        if output.bytes.is_null() {
            return Err("Windows returned empty protected data.".into());
        }
        let result = unsafe {
            let bytes = std::slice::from_raw_parts(output.bytes, output.size as usize).to_vec();
            LocalFree(output.bytes as *mut c_void);
            bytes
        };
        Ok(result)
    }
    pub(super) fn protect(clear: &[u8]) -> Result<Vec<u8>, String> {
        transform(clear, true)
    }
    pub(super) fn unprotect(cipher: &[u8]) -> Result<Vec<u8>, String> {
        transform(cipher, false)
    }
}
pub fn load(path: &Path) -> Result<HashMap<Provider, String>, String> {
    if !path.exists() {
        return Ok(HashMap::new());
    }
    #[cfg(windows)]
    {
        let encrypted = fs::read(path).map_err(|e| format!("Cannot read saved AI key: {e}"))?;
        let clear = dpapi::unprotect(&encrypted)?;
        serde_json::from_slice(&clear).map_err(|_| "Stored AI credentials are invalid.".into())
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Ok(HashMap::new())
    }
}
pub fn save(path: &Path, keys: &HashMap<Provider, String>) -> Result<(), String> {
    #[cfg(windows)]
    {
        let raw = serde_json::to_vec(keys).map_err(|e| e.to_string())?;
        let encrypted = dpapi::protect(&raw)?;
        let parent = path.parent().ok_or("Invalid credential storage path.")?;
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, &encrypted).map_err(|e| e.to_string())?;
        if let Err(e) = fs::rename(&tmp, path) {
            let _ = fs::remove_file(&tmp);
            return Err(format!("Could not update protected key file: {e}"));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = (path, keys);
        Err("Saved API credentials require Windows DPAPI.".into())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    #[test]
    fn encrypted_file_survives_replacement_and_can_be_cleared() {
        let path =
            std::env::temp_dir().join(format!("worldkiln_ai_dpapi_{}.dat", std::process::id()));
        let mut keys = HashMap::new();
        keys.insert(Provider::Gemini, "key-never-plaintext-on-disk".into());
        save(&path, &keys).unwrap();
        assert!(
            !String::from_utf8_lossy(&fs::read(&path).unwrap()).contains("key-never-plaintext")
        );
        assert_eq!(load(&path).unwrap(), keys);
        keys.insert(Provider::OpenAi, "second-key".into());
        save(&path, &keys).unwrap();
        assert_eq!(load(&path).unwrap(), keys);
        keys.remove(&Provider::Gemini);
        save(&path, &keys).unwrap();
        assert_eq!(load(&path).unwrap(), keys);
        fs::remove_file(path).unwrap();
    }
    #[cfg(windows)]
    #[test]
    fn windows_key_round_trip_does_not_expose_plaintext() {
        let mut keys = HashMap::new();
        keys.insert(Provider::Gemini, "example-confidential-test-123".to_owned());
        let source = serde_json::to_vec(&keys).unwrap();
        let encrypted = dpapi::protect(&source).unwrap();
        assert!(!String::from_utf8_lossy(&encrypted).contains("example-confidential-test-123"));
        let recovered = dpapi::unprotect(&encrypted).unwrap();
        assert_eq!(
            serde_json::from_slice::<HashMap<Provider, String>>(&recovered).unwrap(),
            keys
        );
    }
}
