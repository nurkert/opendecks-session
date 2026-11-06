//! Best-effort USB device enumeration, used only as additional information
//! for device matching (MIDI port names are the primary key).

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsbDevice {
    pub vendor_id: u16,
    pub product_id: u16,
    pub product: Option<String>,
}

/// Lists attached USB devices. Returns an empty list where unsupported.
pub fn list_usb_devices() -> Vec<UsbDevice> {
    #[cfg(target_os = "linux")]
    {
        linux()
    }
    #[cfg(target_os = "macos")]
    {
        macos()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        Vec::new()
    }
}

#[cfg(target_os = "linux")]
fn linux() -> Vec<UsbDevice> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir("/sys/bus/usb/devices") else { return out };
    for e in rd.flatten() {
        let p = e.path();
        let read = |f: &str| std::fs::read_to_string(p.join(f)).ok().map(|s| s.trim().to_string());
        let (Some(v), Some(pid)) = (read("idVendor"), read("idProduct")) else { continue };
        let (Ok(v), Ok(pid)) = (u16::from_str_radix(&v, 16), u16::from_str_radix(&pid, 16)) else {
            continue;
        };
        out.push(UsbDevice { vendor_id: v, product_id: pid, product: read("product") });
    }
    out
}

#[cfg(target_os = "macos")]
fn macos() -> Vec<UsbDevice> {
    let Ok(o) = std::process::Command::new("/usr/sbin/ioreg").args(["-p", "IOUSB", "-l", "-w0"]).output()
    else {
        return Vec::new();
    };
    parse_ioreg(&String::from_utf8_lossy(&o.stdout))
}

/// Parses `ioreg -p IOUSB -l` output.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn parse_ioreg(text: &str) -> Vec<UsbDevice> {
    let mut out = Vec::new();
    let mut cur: (Option<u16>, Option<u16>, Option<String>) = (None, None, None);
    let flush = |cur: &mut (Option<u16>, Option<u16>, Option<String>), out: &mut Vec<UsbDevice>| {
        if let (Some(v), Some(p)) = (cur.0, cur.1) {
            out.push(UsbDevice { vendor_id: v, product_id: p, product: cur.2.clone() });
        }
        *cur = (None, None, None);
    };
    for line in text.lines() {
        if line.contains("+-o ") {
            flush(&mut cur, &mut out);
            continue;
        }
        let Some((k, v)) = line.split_once(" = ") else { continue };
        let k = k.trim().trim_start_matches('|').trim().trim_matches('"');
        let v = v.trim();
        match k {
            "idVendor" => cur.0 = v.parse().ok(),
            "idProduct" => cur.1 = v.parse().ok(),
            "USB Product Name" => cur.2 = Some(v.trim_matches('"').to_string()),
            _ => {}
        }
    }
    flush(&mut cur, &mut out);
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_ioreg() {
        let t = r#"+-o Root
  +-o DDJ-FLX10@01100000  <class IOUSBHostDevice>
    | {
    |   "USB Product Name" = "DDJ-FLX10"
    |   "idProduct" = 65
    |   "idVendor" = 11123
    | }
"#;
        let d = super::parse_ioreg(t);
        assert_eq!(d.len(), 1);
        assert_eq!((d[0].vendor_id, d[0].product_id), (0x2B73, 0x0041));
        assert_eq!(d[0].product.as_deref(), Some("DDJ-FLX10"));
    }
}
