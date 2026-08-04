//! Traceroute: đường đi mạng tới mục tiêu, mỗi hop kèm RTT.
//!
//! Cách làm — **shell tới `traceroute`/`tracert`** của hệ thống thay vì tự viết
//! bằng raw socket:
//!
//! * `traceroute` trên macOS/BSD được setuid root nên chạy được từ user thường,
//!   và nó biết cách gửi UDP/ICMP kèm TTL rồi đọc bản trả lời qua raw socket —
//!   thứ mà một Space App không nên (và không được) đòi hỏi.
//! * Tự viết bằng `IP_TTL` + `TCP connect` **không nhận được ICMP TTL Exceeded**
//!   nếu không mở raw socket. Kết quả chỉ đo được RTT tới đích, không đọc được
//!   IP của hop trung gian — đúng cái ta cần lại chính là cái đó.
//!
//! Timeouts phòng thủ:
//!
//! * `-w 2` — 2s chờ mỗi query.
//! * `-q 1` — 1 lần thăm mỗi hop (mặc định 3), giảm tổng thời gian 3 lần.
//! * `-m N` — trần TTL, mặc định 30.
//!
//! Với route ~15 hop, một lần chạy ~15-30 giây. Trần trường hợp xấu = `-m` × `-w`.

use anyhow::{anyhow, Result};
use std::net::IpAddr;
use std::process::Stdio;
use std::time::Duration;

/// Trần TTL cho một lần traceroute. 30 là mặc định của `traceroute` — route dài
/// hơn hầu như luôn có hop lặp và không có nghĩa để đọc.
pub const MAX_HOPS: u8 = 30;

#[derive(Debug, Clone)]
pub struct Hop {
    /// TTL của gói phát ra khi hop trả lời — cũng là vị trí trên đường đi.
    pub ttl: u8,
    /// `None` = hop không trả (tường lửa bỏ ICMP, hoặc gói lạc). Đọc bản in ra
    /// dưới dạng "* * *". Không nhầm với hop mã hoá IP về 0.
    pub ip: Option<IpAddr>,
    /// Round-trip time tính theo ms. `None` khi hop không trả lời.
    pub rtt_ms: Option<f64>,
}

/// Chạy `traceroute` và trả về danh sách hop theo thứ tự TTL tăng dần.
///
/// `host` là mục tiêu cuối cùng — nhận cả tên miền lẫn IP. Traceroute tự phân
/// giải; app đã kiểm SSRF ở tầng trên trước khi tới đây.
pub async fn run(host: &str, max_hops: u8, timeout_per_hop: Duration) -> Result<Vec<Hop>> {
    let bin = find_binary()?;
    let secs = timeout_per_hop.as_secs().clamp(1, 10);
    let hops_arg = max_hops.min(MAX_HOPS).to_string();

    // `-n`: không tra ngược DNS (mình sẽ tra riêng, cho phép chọn resolver).
    // `-q 1`: một lần thăm mỗi hop — traceroute mặc định 3, chậm ba lần vô ích.
    // `-w`: chờ tối đa từng gói. `-m`: trần TTL.
    let mut cmd = tokio::process::Command::new(&bin);
    cmd.args([
        "-n",
        "-q", "1",
        "-w", &secs.to_string(),
        "-m", &hops_arg,
        host,
    ])
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());

    // Trần cứng: (max_hops × secs) + biên. Vượt là traceroute treo, phải cắt.
    let hard_cap = Duration::from_secs((max_hops as u64) * secs + 5);
    let output = tokio::time::timeout(hard_cap, cmd.output())
        .await
        .map_err(|_| anyhow!("traceroute quá hạn ({}s)", hard_cap.as_secs()))?
        .map_err(|e| anyhow!("không chạy được {}: {e}", bin))?;

    if !output.status.success() && output.stdout.is_empty() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("traceroute lỗi: {}", err.trim()));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(parse(&stdout))
}

/// Tìm binary theo hệ điều hành. Không tìm thấy → nói rõ, đừng để lỗi mù mờ.
fn find_binary() -> Result<String> {
    for c in ["/usr/sbin/traceroute", "/usr/bin/traceroute", "traceroute"] {
        // `traceroute` trên PATH cũng thử, để hỗ trợ nix / bản build custom.
        if std::path::Path::new(c).exists() || c == "traceroute" {
            return Ok(c.to_string());
        }
    }
    Err(anyhow!(
        "không tìm thấy binary `traceroute`. macOS/BSD có sẵn ở /usr/sbin/traceroute; \
         Linux cài bằng gói `traceroute` (Debian/Ubuntu: apt install traceroute)."
    ))
}

/// Phân tích stdout của traceroute (biến thể BSD/macOS **và** Linux GNU).
///
/// Hai định dạng đầu vào ta cần hỗ trợ:
///
/// * BSD: ` 1  10.0.0.1  5.123 ms`
/// * BSD (không trả): ` 2  * * *` hoặc ` 2  *`
/// * GNU: ` 1  10.0.0.1 (10.0.0.1)  5.123 ms` (khi không có `-n`, tên+IP)
///
/// Tách bằng phép quét thô — không cố dùng regex phức tạp, vì mỗi biến thể lại
/// bổ sung một cái ngoặc riêng. Cần nhất là **không được đọc `*` thành hop tồn tại**.
pub fn parse(output: &str) -> Vec<Hop> {
    let mut hops = vec![];
    for line in output.lines() {
        let t = line.trim();
        // Dòng đầu "traceroute to 1.1.1.1 (1.1.1.1)..." bỏ qua.
        if t.is_empty() || !t.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            continue;
        }
        let mut it = t.split_whitespace();
        let Some(ttl) = it.next().and_then(|s| s.parse::<u8>().ok()) else {
            continue;
        };
        // Đọc token đầu tiên có nghĩa: IP, hoặc `*`.
        let rest: Vec<&str> = it.collect();
        // "* * *" hoặc "*" → hop im lặng.
        if rest.iter().all(|s| *s == "*") {
            hops.push(Hop { ttl, ip: None, rtt_ms: None });
            continue;
        }
        // Tìm token IP đầu tiên
        let ip = rest.iter().find_map(|s| {
            let cleaned = s.trim_start_matches('(').trim_end_matches(')');
            cleaned.parse::<IpAddr>().ok()
        });
        // Tìm số RTT (đứng ngay trước "ms")
        let rtt = rest.windows(2).find_map(|w| {
            (w[1] == "ms").then(|| w[0].parse::<f64>().ok()).flatten()
        });
        hops.push(Hop { ttl, ip, rtt_ms: rtt });
    }
    hops
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_macos_bsd_output_format() {
        // Copy từ chạy thật `traceroute -n -m 5 -q 1 -w 2 1.1.1.1`
        let sample = "\
traceroute to 1.1.1.1 (1.1.1.1), 5 hops max, 40 byte packets
 1  *
 2  172.20.21.254  56.166 ms
 3  172.20.22.3  58.679 ms
 4  *
 5  89.187.163.221  59.096 ms
";
        let h = parse(sample);
        assert_eq!(h.len(), 5);
        assert_eq!(h[0].ttl, 1);
        assert!(h[0].ip.is_none() && h[0].rtt_ms.is_none(), "hop im lặng phải None");
        assert_eq!(h[1].ip, Some("172.20.21.254".parse().unwrap()));
        assert!((h[1].rtt_ms.unwrap() - 56.166).abs() < 0.01);
        assert_eq!(h[4].ip, Some("89.187.163.221".parse().unwrap()));
    }

    #[test]
    fn parses_the_gnu_linux_output_with_name_and_paren_ip() {
        // Định dạng khi không có -n
        let sample = "\
traceroute to google.com (172.217.163.46), 30 hops max, 60 byte packets
 1  _gateway (10.0.0.1)  0.512 ms
 2  * * *
 3  hop-of-doom.isp.net (198.51.100.5)  8.234 ms
";
        let h = parse(sample);
        assert_eq!(h.len(), 3);
        assert_eq!(h[0].ip, Some("10.0.0.1".parse().unwrap()));
        assert!(h[1].ip.is_none());
        assert_eq!(h[2].ip, Some("198.51.100.5".parse().unwrap()));
    }

    #[test]
    fn a_triple_star_hop_is_not_read_as_a_valid_ip() {
        // Bẫy đáng lo nhất: nếu đọc `* * *` thành `0.0.0.0` hoặc bỏ qua thầm
        // lặng thì tuyến đường trông ngắn hơn thực tế.
        let h = parse(" 4  * * *\n 5  * *\n");
        assert_eq!(h.len(), 2);
        for hop in &h {
            assert!(hop.ip.is_none(), "* phải map về None, không phải IP");
            assert!(hop.rtt_ms.is_none());
        }
    }

    #[test]
    fn header_and_non_hop_lines_are_ignored() {
        let sample = "\
traceroute to 1.1.1.1 (1.1.1.1), 30 hops max
oops something weird
 1  10.0.0.1  1.5 ms
";
        let h = parse(sample);
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].ttl, 1);
    }

    #[test]
    fn ipv6_hops_are_parsed_too() {
        let sample = " 1  2001:db8::1  0.5 ms\n 2  2606:4700::1111  10.234 ms\n";
        let h = parse(sample);
        assert_eq!(h.len(), 2);
        assert_eq!(h[0].ip, Some("2001:db8::1".parse().unwrap()));
        assert_eq!(h[1].ip, Some("2606:4700::1111".parse().unwrap()));
    }

    #[test]
    fn rtt_is_extracted_from_the_token_before_ms() {
        // Đơn vị chỉ có ở ms; đừng đọc chỉ số khác thành rtt.
        let h = parse(" 3  8.8.8.8  12.345 ms\n");
        assert_eq!(h[0].rtt_ms.unwrap(), 12.345);
        // Không có "ms" → không có rtt
        let empty = parse(" 3  8.8.8.8  something-else\n");
        assert!(empty[0].rtt_ms.is_none());
        assert_eq!(empty[0].ip, Some("8.8.8.8".parse().unwrap()));
    }
}
