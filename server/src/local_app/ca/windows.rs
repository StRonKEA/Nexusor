//! Implements Windows-specific certificate authority integration.
//! Native Windows user-level root-store access without external command-line tools.

use std::{ffi::c_void, io, ptr, slice};

use windows_sys::Win32::Security::Cryptography::{
    CertCloseStore, CertEnumCertificatesInStore, CertOpenStore, CERT_STORE_OPEN_EXISTING_FLAG,
    CERT_STORE_PROV_SYSTEM_W, CERT_STORE_READONLY_FLAG, CERT_SYSTEM_STORE_CURRENT_USER,
};

use crate::{Error, Result};

const ROOT_STORE: [u16; 5] = [b'R' as u16, b'O' as u16, b'O' as u16, b'T' as u16, 0];

pub(super) fn is_installed(cert: &str) -> Result<bool> {
    let der = certificate_der(cert)?;
    let store = open_root_store()?;
    let mut context = ptr::null();
    let mut found = false;
    loop {
        context = unsafe { CertEnumCertificatesInStore(store, context) };
        if context.is_null() {
            break;
        }
        let encoded = unsafe {
            slice::from_raw_parts((*context).pbCertEncoded, (*context).cbCertEncoded as usize)
        };
        if encoded == der {
            found = true;
            break;
        }
    }
    if !context.is_null() {
        unsafe { windows_sys::Win32::Security::Cryptography::CertFreeCertificateContext(context) };
    }
    close_store(store)?;
    Ok(found)
}

fn certificate_der(cert: &str) -> Result<Vec<u8>> {
    pem::parse(cert)
        .map(|pem| pem.into_contents())
        .map_err(|error| Error::Config(format!("parse CA PEM: {error}")))
}

fn open_root_store() -> Result<*mut c_void> {
    let flags =
        CERT_SYSTEM_STORE_CURRENT_USER | CERT_STORE_OPEN_EXISTING_FLAG | CERT_STORE_READONLY_FLAG;
    let store = unsafe {
        CertOpenStore(
            CERT_STORE_PROV_SYSTEM_W,
            0,
            0,
            flags,
            ROOT_STORE.as_ptr().cast(),
        )
    };
    if store.is_null() {
        return Err(Error::Config(format!(
            "open Windows CurrentUser Root store: {}",
            io::Error::last_os_error()
        )));
    }
    Ok(store)
}

fn close_store(store: *mut c_void) -> Result<()> {
    if unsafe { CertCloseStore(store, 0) } == 0 {
        return Err(Error::Config(format!(
            "close Windows certificate store: {}",
            io::Error::last_os_error()
        )));
    }
    Ok(())
}

/// Replaces a file's DACL with owner-only access.
///
/// Uses `icacls` rather than building an ACL by hand: it is present on every
/// supported Windows version, and the grant/deny semantics are the part that is
/// easy to get subtly wrong. Failures are reported rather than ignored, because a
/// readable CA private key defeats the point of restricting it.
pub(super) fn restrict_to_current_user(path: &std::path::Path) -> Result<()> {
    let user = current_user_account()?;
    let target = path.to_string_lossy();
    // Remove inherited and explicit permissions, then grant the owner full
    // control. `:(D)` denies are avoided: an explicit deny that cannot be
    // overridden produces surprising failures for backup tooling.
    let grant = format!("{user}:(F)");
    let status = std::process::Command::new("icacls")
        .arg(&*target)
        .arg("/inheritance:r")
        .arg("/grant")
        .arg(&grant)
        .output()
        .map_err(|error| {
            Error::Config(format!(
                "cannot restrict permissions on the CA private key: {error}"
            ))
        })?;
    if !status.status.success() {
        return Err(Error::Config(format!(
            "cannot restrict permissions on the CA private key: {}",
            String::from_utf8_lossy(&status.stderr).trim()
        )));
    }
    Ok(())
}

/// The current user's account name in `DOMAIN\name` form, which is what `icacls` expects.
fn current_user_account() -> Result<String> {
    let user = std::env::var("USERNAME")
        .map_err(|_| Error::Config("cannot resolve the current user name".into()))?;
    let domain = std::env::var("USERDOMAIN").unwrap_or_default();
    Ok(if domain.is_empty() {
        user
    } else {
        format!("{domain}\\{user}")
    })
}
