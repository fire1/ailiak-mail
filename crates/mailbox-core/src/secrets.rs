use anyhow::{Context, Result};
use keyring::Entry;

const SERVICE: &str = "mailbox";

fn entry(key: &str) -> Result<Entry> {
    Entry::new(SERVICE, key).context("open keyring entry")
}

pub fn set_mail_password(account_id: &str, password: &str) -> Result<()> {
    entry(&format!("mail_password_{account_id}"))?
        .set_password(password)
        .context("store mail password in keyring")
}

pub fn get_mail_password(account_id: &str) -> Result<Option<String>> {
    match entry(&format!("mail_password_{account_id}"))?.get_password() {
        Ok(p) => Ok(Some(p)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e).context("read mail password from keyring"),
    }
}

pub fn delete_mail_password(account_id: &str) -> Result<()> {
    match entry(&format!("mail_password_{account_id}"))?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e).context("delete mail password"),
    }
}

pub fn set_ailiak_token(token: &str) -> Result<()> {
    entry("ailiak_token")?
        .set_password(token)
        .context("store Ailiak token in keyring")
}

pub fn get_ailiak_token() -> Result<Option<String>> {
    match entry("ailiak_token")?.get_password() {
        Ok(p) => Ok(Some(p)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e).context("read Ailiak token from keyring"),
    }
}

pub fn delete_ailiak_token() -> Result<()> {
    match entry("ailiak_token")?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e).context("delete Ailiak token"),
    }
}
