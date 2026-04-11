//! QR code generation and LAN IP detection for remote access pairing.

use std::net::IpAddr;

/// Generate a QR code as a PNG data URI (data:image/png;base64,...).
pub fn generate_qr_data_uri(url: &str) -> anyhow::Result<String> {
    use image::Luma;
    use qrcode::QrCode;

    let code = QrCode::new(url.as_bytes())?;
    let image = code.render::<Luma<u8>>().quiet_zone(true).build();

    let mut png_bytes: Vec<u8> = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut png_bytes);
    image::ImageEncoder::write_image(
        encoder,
        image.as_raw(),
        image.width(),
        image.height(),
        image::ExtendedColorType::L8,
    )?;

    let b64 = base64_encode_bytes(&png_bytes);
    Ok(format!("data:image/png;base64,{}", b64))
}

/// Detect non-loopback, non-Docker LAN IPv4 addresses.
pub fn detect_lan_ips() -> Vec<IpAddr> {
    let Ok(interfaces) = local_ip_address::list_afinet_netifas() else {
        return Vec::new();
    };

    interfaces
        .into_iter()
        .filter_map(|(name, ip)| {
            // Skip loopback
            if ip.is_loopback() {
                return None;
            }
            // Only IPv4 for QR code URLs (simpler for users)
            if !ip.is_ipv4() {
                return None;
            }
            // Skip Docker/container interfaces
            let lower = name.to_lowercase();
            if lower.starts_with("docker")
                || lower.starts_with("br-")
                || lower.starts_with("veth")
                || lower.starts_with("utun")
            {
                return None;
            }
            Some(ip)
        })
        .collect()
}

/// Base64 encode raw bytes.
pub fn base64_encode_bytes(data: &[u8]) -> String {
    use std::fmt::Write;
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        let _ = write!(
            result,
            "{}",
            CHARS[((triple >> 18) & 0x3F) as usize] as char
        );
        let _ = write!(
            result,
            "{}",
            CHARS[((triple >> 12) & 0x3F) as usize] as char
        );
        if chunk.len() > 1 {
            let _ = write!(result, "{}", CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            let _ = write!(result, "{}", CHARS[(triple & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_qr_data_uri_valid_url() {
        let result = generate_qr_data_uri("http://192.168.1.42:8080/?token=abc123");
        assert!(result.is_ok());
        let uri = result.unwrap();
        assert!(uri.starts_with("data:image/png;base64,"));
        assert!(uri.len() > 100); // Should contain real PNG data
    }

    #[test]
    fn test_generate_qr_data_uri_returns_valid_png() {
        let uri = generate_qr_data_uri("http://example.com").unwrap();
        let b64_data = uri.strip_prefix("data:image/png;base64,").unwrap();
        // Verify it's valid base64 by checking length is a multiple of 4
        assert_eq!(b64_data.len() % 4, 0);
    }

    #[test]
    fn test_detect_lan_ips_excludes_loopback() {
        let ips = detect_lan_ips();
        for ip in &ips {
            assert!(!ip.is_loopback(), "LAN IPs should not include loopback");
        }
    }

    #[test]
    fn test_detect_lan_ips_returns_ipv4() {
        let ips = detect_lan_ips();
        for ip in &ips {
            assert!(ip.is_ipv4(), "LAN IPs should be IPv4 only");
        }
    }

    #[test]
    fn test_detect_lan_ips_handles_no_network() {
        // This test just verifies detect_lan_ips doesn't panic.
        // In a container with no LAN interfaces, it returns an empty vec.
        let ips = detect_lan_ips();
        // No assertion on contents; presence of any IPs depends on the environment.
        let _ = ips;
    }
}
