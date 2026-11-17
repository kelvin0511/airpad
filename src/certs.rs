use crate::config::app_data_dir;
use axum_server::tls_rustls::RustlsConfig;
use rcgen::{
    BasicConstraints, Certificate, CertificateParams, DistinguishedName, DnType,
    ExtendedKeyUsagePurpose, IsCa, KeyPair, KeyUsagePurpose, SanType,
};
use std::fs;
use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;

pub struct GeneratedTlsBundle {
    pub rustls_config: RustlsConfig,
    pub ca_der: Vec<u8>,
    pub mobileconfig: Vec<u8>,
}

fn certs_dir() -> PathBuf {
    app_data_dir().join("certs")
}

fn load_or_create_ca(dir: &PathBuf) -> Result<(Certificate, KeyPair, Vec<u8>), String> {
    fs::create_dir_all(dir).map_err(|e| format!("failed to create certs dir: {e}"))?;
    let ca_cert_path = dir.join("airpad_ca.pem");
    let ca_key_path = dir.join("airpad_ca_key.pem");

    if let (Ok(cert_pem), Ok(key_pem)) = (
        fs::read_to_string(&ca_cert_path),
        fs::read_to_string(&ca_key_path),
    ) {
        if let (Ok(ca_key), Ok(ca_params)) = (
            KeyPair::from_pem(&key_pem),
            CertificateParams::from_ca_cert_pem(&cert_pem),
        ) {
            if let Ok(ca_cert) = ca_params.self_signed(&ca_key) {
                let der = ca_cert.der().to_vec();
                return Ok((ca_cert, ca_key, der));
            }
        }
    }

    let mut params = CertificateParams::default();
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, "AirPad Local Root CA");
    dn.push(DnType::OrganizationName, "AirPad");
    params.distinguished_name = dn;
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];

    let ca_key = KeyPair::generate().map_err(|e| format!("CA keygen failed: {e}"))?;
    let ca_cert = params
        .self_signed(&ca_key)
        .map_err(|e| format!("CA self-sign failed: {e}"))?;

    let _ = fs::write(&ca_cert_path, ca_cert.pem());
    let _ = fs::write(&ca_key_path, ca_key.serialize_pem());

    let der = ca_cert.der().to_vec();
    Ok((ca_cert, ca_key, der))
}

pub async fn build_tls_bundle(lan_ips: &[Ipv4Addr]) -> Result<GeneratedTlsBundle, String> {
    let dir = certs_dir();
    let (ca_cert, ca_key, ca_der) = load_or_create_ca(&dir)?;

    let mut leaf_params = CertificateParams::default();
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, "AirPad Server");
    dn.push(DnType::OrganizationName, "AirPad");
    leaf_params.distinguished_name = dn;
    leaf_params.is_ca = IsCa::NoCa;
    leaf_params.key_usages = vec![
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::KeyEncipherment,
    ];
    leaf_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];

    leaf_params
        .subject_alt_names
        .push(SanType::IpAddress(IpAddr::V4(Ipv4Addr::LOCALHOST)));
    if let Ok(dns) = "localhost".try_into() {
        leaf_params.subject_alt_names.push(SanType::DnsName(dns));
    }
    for &ip in lan_ips {
        leaf_params
            .subject_alt_names
            .push(SanType::IpAddress(IpAddr::V4(ip)));
    }

    let leaf_key = KeyPair::generate().map_err(|e| format!("leaf keygen failed: {e}"))?;
    let leaf_cert = leaf_params
        .signed_by(&leaf_key, &ca_cert, &ca_key)
        .map_err(|e| format!("leaf cert signing failed: {e}"))?;

    let cert_chain_pem = format!("{}\n{}", leaf_cert.pem(), ca_cert.pem());
    let key_pem = leaf_key.serialize_pem();

    let rustls_config = RustlsConfig::from_pem(cert_chain_pem.into_bytes(), key_pem.into_bytes())
        .await
        .map_err(|e| format!("rustls config failed: {e}"))?;

    let mobileconfig = build_ios_mobileconfig(&ca_der).into_bytes();

    Ok(GeneratedTlsBundle {
        rustls_config,
        ca_der,
        mobileconfig,
    })
}

fn build_ios_mobileconfig(ca_der: &[u8]) -> String {
    let b64 = encode_base64(ca_der);
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>PayloadContent</key>
  <array>
    <dict>
      <key>PayloadCertificateFileName</key>
      <string>airpad-ca.cer</string>
      <key>PayloadContent</key>
      <data>
{b64}
      </data>
      <key>PayloadDescription</key>
      <string>Allows Safari on iPhone to trust your local AirPad PC server for WSS and Gyroscope Air-Mouse.</string>
      <key>PayloadDisplayName</key>
      <string>AirPad Local Root CA</string>
      <key>PayloadIdentifier</key>
      <string>dev.airpad.local.ca</string>
      <key>PayloadType</key>
      <string>com.apple.security.root</string>
      <key>PayloadUUID</key>
      <string>7B6E8144-9E3A-4A5E-9A2B-1A2B3C4D5E6F</string>
      <key>PayloadVersion</key>
      <integer>1</integer>
    </dict>
  </array>
  <key>PayloadDescription</key>
  <string>Installs the AirPad local HTTPS certificate generated on your PC.</string>
  <key>PayloadDisplayName</key>
  <string>AirPad Local HTTPS Trust</string>
  <key>PayloadIdentifier</key>
  <string>dev.airpad.local.profile</string>
  <key>PayloadRemovalDisallowed</key>
  <false/>
  <key>PayloadType</key>
  <string>Configuration</string>
  <key>PayloadUUID</key>
  <string>4A2C9D10-1F8B-4B7E-A2E1-998877665544</string>
  <key>PayloadVersion</key>
  <integer>1</integer>
</dict>
</plist>
"#
    )
}

fn encode_base64(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((input.len() + 2) / 3 * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;

        out.push(TABLE[((n >> 18) & 0x3F) as usize] as char);
        out.push(TABLE[((n >> 12) & 0x3F) as usize] as char);
        out.push(if chunk.len() > 1 {
            TABLE[((n >> 6) & 0x3F) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[(n & 0x3F) as usize] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc_vectors() {
        assert_eq!(encode_base64(b"Man"), "TWFu");
        assert_eq!(encode_base64(b"Ma"), "TWE=");
        assert_eq!(encode_base64(b"M"), "TQ==");
    }

    #[tokio::test]
    async fn builds_tls_bundle_and_mobileconfig() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let bundle = build_tls_bundle(&[Ipv4Addr::new(192, 168, 1, 50)])
            .await
            .expect("TLS bundle should build cleanly");
        assert!(!bundle.ca_der.is_empty());
        let mc = String::from_utf8(bundle.mobileconfig).unwrap();
        assert!(mc.contains("com.apple.security.root"));
        assert!(mc.contains(&encode_base64(&bundle.ca_der)));
    }
}
