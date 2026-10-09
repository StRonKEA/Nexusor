//! Installs and manages the local certificate authority.
use std::{fs, path::PathBuf};

mod windows;

use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, IsCa, Issuer, KeyPair,
    KeyUsagePurpose, RsaKeySize, PKCS_RSA_SHA256,
};
use time::{Duration, OffsetDateTime};
use x509_parser::prelude::FromDer;

use crate::{config::managed_data_dir, Error, Result};

use super::CaState;

#[derive(Clone)]
pub struct CaManager {
    dir: PathBuf,
}

pub struct LoadedCa {
    pub issuer: Issuer<'static, KeyPair>,
}

impl CaManager {
    pub fn managed() -> Result<Self> {
        Ok(Self {
            dir: managed_data_dir()?.join("ca"),
        })
    }

    fn cert_path(&self) -> PathBuf {
        self.dir.join("ca.crt")
    }
    fn key_path(&self) -> PathBuf {
        self.dir.join("ca.key")
    }

    pub fn state(&self) -> Result<CaState> {
        let cert = fs::read_to_string(self.cert_path());
        let key = fs::read_to_string(self.key_path());
        match (cert, key) {
            (Err(cert_error), Err(key_error))
                if cert_error.kind() == std::io::ErrorKind::NotFound
                    && key_error.kind() == std::io::ErrorKind::NotFound =>
            {
                Ok(CaState::Missing)
            }
            (Ok(cert), Ok(key)) => {
                if parse_issuer(&cert, &key).is_err() {
                    return Ok(CaState::Invalid);
                }
                Ok(if is_installed(&cert)? {
                    CaState::Ready
                } else {
                    CaState::Untrusted
                })
            }
            _ => Ok(CaState::Invalid),
        }
    }

    pub fn load(&self) -> Result<LoadedCa> {
        let cert = fs::read_to_string(self.cert_path())?;
        let key = fs::read_to_string(self.key_path())?;
        Ok(LoadedCa {
            issuer: parse_issuer(&cert, &key)?,
        })
    }

    pub fn install_command(&self) -> Option<String> {
        Some(format!(
            "certutil -user -addstore -f Root \"{}\"",
            self.cert_path().display()
        ))
    }

    pub fn initialize_local(&self) -> Result<()> {
        match self.state()? {
            CaState::Invalid => {
                return Err(Error::Config("CA files are incomplete or invalid".into()))
            }
            CaState::Ready => return Ok(()),
            CaState::Missing => self.generate()?,
            CaState::Untrusted => {}
        }
        Ok(())
    }

    fn generate(&self) -> Result<()> {
        fs::create_dir_all(&self.dir)?;

        let key = KeyPair::generate_rsa_for(&PKCS_RSA_SHA256, RsaKeySize::_3072)
            .map_err(|error| Error::Config(format!("generate CA key: {error}")))?;
        let mut params = CertificateParams::new(Vec::<String>::new())
            .map_err(|error| Error::Config(format!("create CA parameters: {error}")))?;
        let mut name = DistinguishedName::new();
        name.push(DnType::CommonName, "Nexusor Local CA");
        name.push(DnType::OrganizationName, "Nexusor");
        params.distinguished_name = name;
        params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
        params.key_usages = vec![
            KeyUsagePurpose::DigitalSignature,
            KeyUsagePurpose::KeyCertSign,
            KeyUsagePurpose::CrlSign,
        ];
        params.not_before = OffsetDateTime::now_utc() - Duration::minutes(5);
        params.not_after = OffsetDateTime::now_utc() + Duration::days(3652);
        let cert = params
            .self_signed(&key)
            .map_err(|error| Error::Config(format!("generate CA certificate: {error}")))?;
        write_private(&self.key_path(), key.serialize_pem().as_bytes())?;
        write_atomic(&self.cert_path(), cert.pem().as_bytes())?;
        Ok(())
    }
}

/// Restricts a file to the owning user.
///
/// The CA private key can mint certificates for any host, so on Windows the
/// default inherited ACL would let any other local account read it. The ACL is
/// tightened on a fresh file before any key material is written, so the key is
/// never briefly readable by others.
#[cfg(windows)]
fn write_private(path: &std::path::Path, data: &[u8]) -> Result<()> {
    use std::io::Write as _;

    // Create empty, lock the ACL down, then write. Doing it in this order means
    // the key bytes never exist under a permissive DACL.
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)?;
    super::ca::windows::restrict_to_current_user(path)?;
    file.write_all(data)?;
    file.sync_all()?;
    Ok(())
}

#[cfg(not(windows))]
fn write_private(path: &std::path::Path, data: &[u8]) -> Result<()> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(data)?;
    Ok(())
}

fn parse_issuer(cert: &str, key: &str) -> Result<Issuer<'static, KeyPair>> {
    let key =
        KeyPair::from_pem(key).map_err(|error| Error::Config(format!("parse CA key: {error}")))?;
    let pem = pem::parse(cert).map_err(|error| Error::Config(format!("parse CA PEM: {error}")))?;
    let (_, parsed) = x509_parser::certificate::X509Certificate::from_der(pem.contents())
        .map_err(|error| Error::Config(format!("parse CA X.509 certificate: {error}")))?;
    if parsed.public_key().subject_public_key.data.as_ref() != key.public_key_raw() {
        return Err(Error::Config(
            "CA certificate and private key do not match".into(),
        ));
    }
    if !parsed.validity().is_valid() {
        return Err(Error::Config(
            "CA certificate is outside its validity period".into(),
        ));
    }
    if !parsed
        .basic_constraints()
        .map_err(|error| Error::Config(format!("read CA constraints: {error}")))?
        .is_some_and(|constraints| constraints.value.ca)
    {
        return Err(Error::Config("certificate is not a CA".into()));
    }
    Issuer::from_ca_cert_pem(cert, key)
        .map_err(|error| Error::Config(format!("parse CA certificate: {error}")))
}

fn write_atomic(path: &std::path::Path, data: &[u8]) -> Result<()> {
    let temp = path.with_extension("tmp");
    fs::write(&temp, data)?;
    fs::rename(&temp, path)?;
    Ok(())
}

fn is_installed(cert: &str) -> Result<bool> {
    windows::is_installed(cert)
}
