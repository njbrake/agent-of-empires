//! QR code generation and LAN IP detection for remote access pairing.

use std::net::IpAddr;

/// A detected network interface suitable for remote access pairing.
#[derive(Debug, Clone)]
pub struct DetectedInterface {
    /// Interface name (e.g. "en0", "utun3") — used for the UI label.
    pub name: String,
    /// IPv4 address.
    pub ip: IpAddr,
    /// Human-friendly label: "Wi-Fi", "Tailscale", "Ethernet", etc.
    pub label: String,
}

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

/// Detect network interfaces usable for remote pairing.
///
/// Includes physical LAN (en*, eth*) AND VPN tunnels (utun*, tun*) since
/// Tailscale, WireGuard, etc. are valid pairing targets. Skips loopback,
/// Docker bridges, and link-local auto-config addresses.
pub fn detect_interfaces() -> Vec<DetectedInterface> {
    let Ok(interfaces) = local_ip_address::list_afinet_netifas() else {
        return Vec::new();
    };

    let mut out: Vec<DetectedInterface> = interfaces
        .into_iter()
        .filter_map(|(name, ip)| {
            if ip.is_loopback() {
                return None;
            }
            if !ip.is_ipv4() {
                return None;
            }
            // Skip link-local auto-config (169.254.x.x)
            if let IpAddr::V4(v4) = ip {
                if v4.is_link_local() {
                    return None;
                }
            }
            // Skip Docker/container bridges — not reachable from phones
            let lower = name.to_lowercase();
            if lower.starts_with("docker") || lower.starts_with("br-") || lower.starts_with("veth")
            {
                return None;
            }
            let label = label_for_interface(&name);
            Some(DetectedInterface { name, ip, label })
        })
        .collect();

    // Sort so physical LAN comes first (most common case), VPNs second.
    out.sort_by_key(|iface| {
        let lname = iface.name.to_lowercase();
        if lname.starts_with("en") || lname.starts_with("eth") || lname.starts_with("wlan") {
            0
        } else if lname.starts_with("utun") || lname.starts_with("tun") {
            1
        } else {
            2
        }
    });
    out
}

/// Map an interface name to a human-friendly label.
fn label_for_interface(name: &str) -> String {
    let lower = name.to_lowercase();
    // macOS: en0 is usually Wi-Fi on laptops, en1/en2 are Ethernet/Thunderbolt
    if lower == "en0" {
        "Wi-Fi".to_string()
    } else if lower.starts_with("en") {
        format!("Ethernet ({})", name)
    } else if lower.starts_with("utun") || lower.starts_with("tun") {
        // utun on macOS is most commonly Tailscale, but could be WireGuard/OpenVPN.
        // We can't know for sure without more introspection.
        format!("VPN / Tailscale ({})", name)
    } else if lower.starts_with("wlan") {
        "Wi-Fi".to_string()
    } else if lower.starts_with("eth") {
        format!("Ethernet ({})", name)
    } else {
        name.to_string()
    }
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
    fn test_detect_interfaces_excludes_loopback() {
        let ifaces = detect_interfaces();
        for iface in &ifaces {
            assert!(!iface.ip.is_loopback(), "should exclude loopback");
        }
    }

    #[test]
    fn test_detect_interfaces_returns_ipv4() {
        let ifaces = detect_interfaces();
        for iface in &ifaces {
            assert!(iface.ip.is_ipv4(), "should be IPv4 only");
        }
    }

    #[test]
    fn test_detect_interfaces_handles_no_network() {
        // Just verify it doesn't panic.
        let _ = detect_interfaces();
    }

    #[test]
    fn test_label_for_interface_en0_is_wifi() {
        assert_eq!(label_for_interface("en0"), "Wi-Fi");
    }

    #[test]
    fn test_label_for_interface_utun_is_vpn() {
        assert_eq!(label_for_interface("utun3"), "VPN / Tailscale (utun3)");
    }
}
