//! Tra địa chỉ MAC từ ARP cache của hệ điều hành.
//!
//! **Ranh giới vật lý cần nói ngay:** MAC là địa chỉ **lớp 2**, chỉ tồn tại giữa
//! hai thiết bị trên **cùng một segment mạng**. Mỗi lần gói qua router (L3), MAC
//! nguồn/đích được viết lại thành MAC của cặp giao diện router — MAC nguyên bản
//! bị bỏ. Vì vậy **không thể** biết MAC của một máy chủ ở xa trên Internet, và
//! **không thể** biết MAC của một hop trung gian nào không cùng LAN với máy chạy
//! app. Đây không phải giới hạn của công cụ, đây là cách IP hoạt động.
//!
//! App **chỉ trả MAC khi thật sự đọc được từ ARP cache local** (nghĩa là mục
//! tiêu cùng LAN). Với hop xa, trả None và nói vì sao — không bao giờ đoán.

use std::net::IpAddr;
use std::process::Stdio;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mac {
    /// Chuỗi MAC chuẩn hoá: `aa:bb:cc:dd:ee:ff` (thường, hai chữ số + :).
    pub addr: String,
    /// Giao diện mạng đã nhìn thấy MAC này — hữu ích để biết đây là LAN nào.
    pub iface: Option<String>,
    /// Từ đâu ra: `arp -n` (mac/BSD), `/proc/net/arp` (Linux), `ip neigh`, …
    pub source: &'static str,
}

impl Mac {
    /// Ánh xạ 3 byte đầu → tên vendor. Chỉ vài prefix hay gặp cho tiện đọc;
    /// dữ liệu OUI đầy đủ ~30k dòng không nên đóng vào binary. `None` = không biết.
    pub fn vendor_hint(&self) -> Option<&'static str> {
        let bytes: Vec<&str> = self.addr.split(':').take(3).collect();
        if bytes.len() != 3 {
            return None;
        }
        let oui = bytes.join(":").to_ascii_lowercase();
        for (prefix, name) in KNOWN_OUI {
            if oui == *prefix {
                return Some(*name);
            }
        }
        None
    }
}

/// Danh mục OUI mà nhầm chúng dẫn tới kết luận sai về mặt hạ tầng: gateway của
/// nhà cung cấp mạng, VM/hypervisor (nếu MAC là ảo thì "IP này là VM"), IoT.
/// Không đầy đủ có chủ đích — muốn tra vendor thật thì có ieee.org/oui.
const KNOWN_OUI: &[(&str, &str)] = &[
    ("00:0c:29", "VMware ESXi"),
    ("00:50:56", "VMware"),
    ("00:1c:14", "VMware"),
    ("00:15:5d", "Microsoft Hyper-V"),
    ("52:54:00", "QEMU/KVM (libvirt)"),
    ("00:16:3e", "Xen"),
    ("08:00:27", "VirtualBox"),
    ("00:03:ff", "Microsoft (Hyper-V/Azure)"),
    ("10:5a:f7", "Wi-Fi router (Tenda?)"),
    ("dc:a6:32", "Raspberry Pi"),
    ("b8:27:eb", "Raspberry Pi"),
    ("e4:5f:01", "Raspberry Pi"),
];

/// Chuẩn hoá MAC về chữ thường + phân cách `:`. Nhận cả `AA-BB-CC-...` (Windows)
/// và `AABB.CCDD.EEFF` (Cisco). Không hợp lệ → None.
pub fn normalize(raw: &str) -> Option<String> {
    // Chỉ giữ chữ hex, rồi cắt 12 ký tự
    let hex: String = raw
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .take(12)
        .collect();
    if hex.len() != 12 {
        return None;
    }
    let bytes: Vec<String> = (0..12)
        .step_by(2)
        .map(|i| hex[i..i + 2].to_ascii_lowercase())
        .collect();
    Some(bytes.join(":"))
}

/// Phân tích một dòng của `arp -n <ip>` (macOS/BSD):
///
/// `? (10.0.0.1) at aa:bb:cc:dd:ee:ff on en0 ifscope [ethernet]`
///
/// Hoặc: `10.0.0.1 (10.0.0.1) -- no entry`
///
/// Trả `Some(mac)` chỉ khi thật sự có MAC — "no entry" và `(incomplete)` là None.
pub fn parse_bsd_arp(line: &str) -> Option<Mac> {
    let l = line.trim();
    if l.contains("no entry") || l.contains("(incomplete)") {
        return None;
    }
    // Tách theo "at " (giữa IP và MAC)
    let rest = l.split(" at ").nth(1)?;
    let mac_token = rest.split_whitespace().next()?;
    let mac = normalize(mac_token)?;
    let iface = rest
        .split(" on ")
        .nth(1)
        .and_then(|s| s.split_whitespace().next())
        .map(|s| s.to_string());
    Some(Mac {
        addr: mac,
        iface,
        source: "arp -n",
    })
}

/// Đọc một dòng của `/proc/net/arp` (Linux):
///
/// `10.0.0.1  0x1  0x2  aa:bb:cc:dd:ee:ff  *  eth0`
///
/// Trả None cho MAC 00:00:00:00:00:00 (không có entry hợp lệ) — nếu không thì
/// bảo là "IP này có MAC là số 0", một lời nói dối chỉ ai không đọc kỹ mới tin.
pub fn parse_linux_arp(line: &str) -> Option<(IpAddr, Mac)> {
    let cols: Vec<&str> = line.split_whitespace().collect();
    if cols.len() < 6 || cols[0] == "IP" {
        return None;
    }
    let ip: IpAddr = cols[0].parse().ok()?;
    let mac_str = cols[3];
    if mac_str == "00:00:00:00:00:00" {
        return None;
    }
    let mac = normalize(mac_str)?;
    Some((
        ip,
        Mac {
            addr: mac,
            iface: Some(cols[5].to_string()),
            source: "/proc/net/arp",
        },
    ))
}

/// Tra MAC của một IP. Không có trong ARP cache → None (chưa từng nói chuyện
/// với nó ở lớp 2, hoặc nó không ở cùng LAN).
///
/// Không **chủ động** gửi ARP request. Muốn buộc cache có entry thì thao tác
/// gọi hàm này ngay sau khi có kết nối TCP thành công tới `ip` — kernel sẽ tự
/// gửi ARP để mở kết nối và cache lại ngay.
pub async fn lookup(ip: IpAddr) -> Option<Mac> {
    // Thử Linux `/proc/net/arp` trước vì rẻ hơn (đọc file), rồi mới shell ra.
    if let Ok(content) = tokio::fs::read_to_string("/proc/net/arp").await {
        for line in content.lines() {
            if let Some((parsed_ip, mac)) = parse_linux_arp(line) {
                if parsed_ip == ip {
                    return Some(mac);
                }
            }
        }
        return None; // File tồn tại → đã tra hết, không có nghĩa là fallback vô ích
    }

    // BSD/macOS: `arp -n <ip>`
    let out = tokio::process::Command::new("arp")
        .args(["-n", &ip.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    parse_bsd_arp(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mac_is_normalised_to_lowercase_with_colons_across_input_styles() {
        assert_eq!(normalize("aa:bb:cc:dd:ee:ff").as_deref(), Some("aa:bb:cc:dd:ee:ff"));
        assert_eq!(normalize("AA:BB:CC:DD:EE:FF").as_deref(), Some("aa:bb:cc:dd:ee:ff"));
        assert_eq!(normalize("aa-bb-cc-dd-ee-ff").as_deref(), Some("aa:bb:cc:dd:ee:ff"));
        assert_eq!(normalize("AABB.CCDD.EEFF").as_deref(), Some("aa:bb:cc:dd:ee:ff"));
        // hex thiếu → None
        assert!(normalize("aa:bb:cc").is_none());
        assert!(normalize("not-a-mac").is_none());
    }

    #[test]
    fn bsd_arp_output_is_parsed_including_the_interface() {
        // Dòng thật từ `arp -n 192.168.0.1` trên macOS của máy này
        let m = parse_bsd_arp("? (192.168.0.1) at 10:5a:95:fa:ba:54 on en0 ifscope [ethernet]").unwrap();
        assert_eq!(m.addr, "10:5a:95:fa:ba:54");
        assert_eq!(m.iface.as_deref(), Some("en0"));
        assert_eq!(m.source, "arp -n");
    }

    #[test]
    fn a_no_entry_reply_is_not_a_mac() {
        // Bẫy chính: "no entry" không được thành `Mac::default()`.
        assert!(parse_bsd_arp("192.168.5.1 (192.168.5.1) -- no entry").is_none());
        assert!(parse_bsd_arp("? (192.168.0.30) at (incomplete) on en0 ifscope [ethernet]").is_none());
    }

    #[test]
    fn a_hostname_prefix_does_not_break_parsing() {
        // arp -n vẫn ghi `?` nhưng khi user bật DNS thì có host name.
        let m = parse_bsd_arp("router.local (10.0.0.1) at aa:bb:cc:dd:ee:ff on en0 [ethernet]").unwrap();
        assert_eq!(m.addr, "aa:bb:cc:dd:ee:ff");
    }

    #[test]
    fn linux_proc_arp_is_parsed_and_all_zero_mac_is_rejected() {
        let hdr = "IP address       HW type     Flags       HW address            Mask     Device";
        let real = "10.0.0.1         0x1         0x2         aa:bb:cc:dd:ee:ff     *        eth0";
        let zero = "10.0.0.5         0x1         0x0         00:00:00:00:00:00     *        eth0";
        assert!(parse_linux_arp(hdr).is_none());
        let (ip, mac) = parse_linux_arp(real).unwrap();
        assert_eq!(ip, "10.0.0.1".parse::<IpAddr>().unwrap());
        assert_eq!(mac.addr, "aa:bb:cc:dd:ee:ff");
        assert_eq!(mac.iface.as_deref(), Some("eth0"));
        // MAC toàn số 0 nghĩa là chưa có entry hợp lệ — không được đọc thành MAC.
        assert!(parse_linux_arp(zero).is_none());
    }

    #[test]
    fn vendor_hint_flags_common_virtualization_ouis() {
        let vm = Mac {
            addr: "00:0c:29:aa:bb:cc".into(),
            iface: None,
            source: "test",
        };
        assert_eq!(vm.vendor_hint(), Some("VMware ESXi"));
        let pi = Mac {
            addr: "b8:27:eb:11:22:33".into(),
            iface: None,
            source: "test",
        };
        assert_eq!(pi.vendor_hint(), Some("Raspberry Pi"));
        let unknown = Mac {
            addr: "aa:bb:cc:dd:ee:ff".into(),
            iface: None,
            source: "test",
        };
        assert!(unknown.vendor_hint().is_none());
    }
}
