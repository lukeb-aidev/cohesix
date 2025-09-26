// CLASSIFICATION: COMMUNITY
// Filename: secure9p_onboard.rs v0.1
// Author: Lukas Bower
// Date Modified: 2029-09-26

use chrono::Utc;
use clap::Parser;
use cohesix::trace::recorder::event;
use cohesix::{coh_bail, coh_error, CohError};
use rcgen::{
    CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa, KeyPair,
    KeyUsagePurpose, SanType, SerialNumber,
};
use std::convert::TryInto;
use std::fs;
use std::path::PathBuf;
use time::{Duration, OffsetDateTime};

#[derive(Parser, Debug)]
#[command(
    about = "Generate SPIFFE-aligned Secure9P client credentials",
    rename_all = "kebab"
)]
struct Args {
    /// Path to the CA certificate used for signing (PEM encoded).
    #[arg(long)]
    ca_cert: PathBuf,
    /// Path to the CA private key (PEM encoded).
    #[arg(long)]
    ca_key: PathBuf,
    /// SPIFFE identifier for the issued certificate.
    #[arg(long)]
    spiffe_id: String,
    /// Output path for the issued client certificate (PEM encoded).
    #[arg(long)]
    out_cert: PathBuf,
    /// Output path for the issued client private key (PEM encoded).
    #[arg(long)]
    out_key: PathBuf,
    /// Optional common name override for the issued certificate.
    #[arg(long)]
    common_name: Option<String>,
    /// Validity window in days (defaults to 365).
    #[arg(long, default_value_t = 365)]
    valid_days: u32,
}

fn main() -> Result<(), CohError> {
    let args = Args::parse();
    if !args.spiffe_id.starts_with("spiffe://") {
        coh_bail!("SPIFFE ID must start with spiffe://");
    }
    if args.valid_days == 0 {
        coh_bail!("valid-days must be greater than zero");
    }

    let ca_cert_pem = fs::read_to_string(&args.ca_cert)?;
    let ca_key_pem = fs::read_to_string(&args.ca_key)?;
    let ca_key =
        KeyPair::from_pem(&ca_key_pem).map_err(|e| coh_error!("failed to parse CA key: {e}"))?;
    let ca_params = CertificateParams::from_ca_cert_pem(&ca_cert_pem)
        .map_err(|e| coh_error!("failed to parse CA certificate: {e}"))?;
    let ca_cert = ca_params
        .clone()
        .self_signed(&ca_key)
        .map_err(|e| coh_error!("failed to reconstruct CA certificate: {e}"))?;

    let mut params = CertificateParams::default();
    let not_before = OffsetDateTime::now_utc() - Duration::minutes(5);
    let not_after = not_before + Duration::days(args.valid_days.into());
    params.not_before = not_before;
    params.not_after = not_after;
    params.is_ca = IsCa::NoCa;
    params.use_authority_key_identifier_extension = true;
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
    params.key_usages = vec![
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::KeyEncipherment,
    ];

    let common_name = args
        .common_name
        .clone()
        .unwrap_or_else(|| default_common_name(&args.spiffe_id));
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, common_name.clone());
    params.distinguished_name = dn;

    let uri = args
        .spiffe_id
        .clone()
        .try_into()
        .map_err(|_| coh_error!("invalid SPIFFE URI: {}", args.spiffe_id))?;
    params.subject_alt_names.push(SanType::URI(uri));

    let serial_seed = Utc::now().timestamp_nanos_opt().unwrap_or_default();
    let serial_value = if serial_seed <= 0 {
        1
    } else {
        (serial_seed as u128 % u128::from(u64::MAX)).max(1) as u64
    };
    params.serial_number = Some(SerialNumber::from(serial_value));

    let client_key = KeyPair::generate().map_err(|e| coh_error!("failed to generate key: {e}"))?;
    let client_cert = params
        .signed_by(&client_key, &ca_cert, &ca_key)
        .map_err(|e| coh_error!("failed to issue certificate: {e}"))?;

    ensure_parent(&args.out_cert)?;
    ensure_parent(&args.out_key)?;
    fs::write(&args.out_cert, client_cert.pem())?;
    fs::write(&args.out_key, client_key.serialize_pem())?;

    event(
        "secure9p_onboard",
        "certificate_issued",
        &format!(
            "spiffe={} cert={} key={} valid_days={}",
            args.spiffe_id,
            args.out_cert.display(),
            args.out_key.display(),
            args.valid_days
        ),
    );
    println!(
        "Issued Secure9P client certificate for {} -> {}",
        args.spiffe_id,
        args.out_cert.display()
    );
    Ok(())
}

fn ensure_parent(path: &PathBuf) -> Result<(), CohError> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    Ok(())
}

fn default_common_name(spiffe: &str) -> String {
    spiffe
        .rsplit('/')
        .find(|segment| !segment.is_empty())
        .unwrap_or("secure9p-client")
        .to_string()
}
