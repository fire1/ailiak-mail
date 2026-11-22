use anyhow::{Context, Result};
use keyring::Entry;

const SERVICE: &str = "ailiakmail";
const LEGACY_SERVICE: &str = "mailbox";

fn entry(service: &str, key: &str) -> Result<Entry> {
    Entry::new(service, key).context("open keyring entry")
}

pub fn set_mail_password(account_id: &str, password: &str) -> Result<()> {
    let key = format!("mail_password_{account_id}");
    entry(SERVICE, &key)?
        .set_password(password)
        .context("store mail password in keyring")
}

pub fn get_mail_password(account_id: &str) -> Result<Option<String>> {
    let key = format!("mail_password_{account_id}");
    match entry(SERVICE, &key)?.get_password() {
        Ok(p) => Ok(Some(p)),
        Err(keyring::Error::NoEntry) => match entry(LEGACY_SERVICE, &key)?.get_password() {
            Ok(p) => Ok(Some(p)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e).context("read mail password from legacy keyring"),
        },
        Err(e) => Err(e).context("read mail password from keyring"),
    }
}

pub fn delete_mail_password(account_id: &str) -> Result<()> {
    let key = format!("mail_password_{account_id}");
    for service in [SERVICE, LEGACY_SERVICE] {
        match entry(service, &key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => {}
            Err(e) => return Err(e).context("delete mail password"),
        }
    }
    Ok(())
}

pub fn set_ailiak_token(token: &str) -> Result<()> {
    entry(SERVICE, "ailiak_token")?
        .set_password(token)
        .context("store Ailiak token in keyring")
}

pub fn get_ailiak_token() -> Result<Option<String>> {
    match entry(SERVICE, "ailiak_token")?.get_password() {
        Ok(p) => Ok(Some(p)),
        Err(keyring::Error::NoEntry) => {
            match entry(LEGACY_SERVICE, "ailiak_token")?.get_password() {
                Ok(p) => Ok(Some(p)),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(e) => Err(e).context("read Ailiak token from legacy keyring"),
            }
        }
        Err(e) => Err(e).context("read Ailiak token from keyring"),
    }
}

pub fn delete_ailiak_token() -> Result<()> {
    for service in [SERVICE, LEGACY_SERVICE] {
        match entry(service, "ailiak_token")?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => {}
            Err(e) => return Err(e).context("delete Ailiak token"),
        }
    }
    Ok(())
}
