//! Where the Gemini API key lives (FR-SET-07, NFR-07): macOS Keychain, behind
//! a trait so tests never touch the user's real Keychain.

use std::sync::Mutex;

pub trait KeyStore: Send + Sync {
    /// The saved key, or None when there is none.
    fn get(&self) -> Result<Option<String>, String>;
    fn set(&self, key: &str) -> Result<(), String>;
    /// Deleting a key that is not there is fine.
    fn delete(&self) -> Result<(), String>;
}

/// A generic password item in the login Keychain.
pub struct Keychain {
    service: String,
    account: String,
}

/// `errSecItemNotFound`.
const NOT_FOUND: i32 = -25300;

impl Keychain {
    /// SRS 7: service `yokoten`, account `gemini-api-key`.
    pub fn gemini() -> Self {
        Self::new("yokoten", "gemini-api-key")
    }

    pub fn new(service: &str, account: &str) -> Self {
        Self {
            service: service.into(),
            account: account.into(),
        }
    }
}

impl KeyStore for Keychain {
    fn get(&self) -> Result<Option<String>, String> {
        use security_framework::passwords::get_generic_password;
        match get_generic_password(&self.service, &self.account) {
            Ok(bytes) => String::from_utf8(bytes)
                .map(Some)
                .map_err(|_| "key ใน Keychain เสีย ให้บันทึกใหม่".into()),
            Err(e) if e.code() == NOT_FOUND => Ok(None),
            Err(e) => Err(format!("อ่าน Keychain ไม่ได้ ({})", e.code())),
        }
    }

    fn set(&self, key: &str) -> Result<(), String> {
        security_framework::passwords::set_generic_password(
            &self.service,
            &self.account,
            key.as_bytes(),
        )
        .map_err(|e| format!("บันทึกลง Keychain ไม่ได้ ({})", e.code()))
    }

    fn delete(&self) -> Result<(), String> {
        match security_framework::passwords::delete_generic_password(&self.service, &self.account) {
            Err(e) if e.code() != NOT_FOUND => Err(format!("ลบจาก Keychain ไม่ได้ ({})", e.code())),
            _ => Ok(()),
        }
    }
}

/// In-memory stand-in for tests.
#[derive(Default)]
pub struct Memory(Mutex<Option<String>>);

impl Memory {
    pub fn with(key: &str) -> Self {
        Self(Mutex::new(Some(key.into())))
    }

    fn slot(&self) -> std::sync::MutexGuard<'_, Option<String>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl KeyStore for Memory {
    fn get(&self) -> Result<Option<String>, String> {
        Ok(self.slot().clone())
    }

    fn set(&self, key: &str) -> Result<(), String> {
        *self.slot() = Some(key.into());
        Ok(())
    }

    fn delete(&self) -> Result<(), String> {
        *self.slot() = None;
        Ok(())
    }
}
