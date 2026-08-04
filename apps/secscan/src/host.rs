//! Lớp L3 — kiểm cấu hình máy chủ qua SSH. **Chỉ đọc.**
//!
//! Hai nguyên tắc định hình module này:
//!
//! 1. **Không bao giờ giữ thông tin đăng nhập.** Tài sản chỉ lưu `ssh_ref` là
//!    id máy chủ bên app `ssh-manager`; mật khẩu và khoá riêng nằm ở đó, secscan
//!    không đọc và không lưu. Một app bảo mật mà tự cất khoá riêng vào JSON là
//!    tự mâu thuẫn.
//! 2. **Mọi lệnh đều chỉ đọc, và điều đó được test cưỡng chế.** Bảng lệnh nằm
//!    ở một chỗ duy nhất, và có test quét từng lệnh tìm động từ ghi/xoá. Thêm
//!    một lệnh có `rm`/`chmod`/`>` là test đỏ.

use crate::db::Finding;
use crate::vuln::Package;
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::collections::HashMap;

/// Bảng lệnh — nguồn sự thật duy nhất về những gì L3 chạy trên máy người dùng.
///
/// `(khoá, lệnh)`. Lệnh nào không có trên máy đích thì trả rỗng, không sao.
pub const COMMANDS: &[(&str, &str)] = &[
    ("os_release", "cat /etc/os-release 2>/dev/null || true"),
    ("sshd_config", "sshd -T 2>/dev/null || cat /etc/ssh/sshd_config 2>/dev/null || true"),
    ("packages_deb", "dpkg-query -W -f='${Package} ${Version}\\n' 2>/dev/null || true"),
    ("packages_rpm", "rpm -qa --qf '%{NAME} %{VERSION}-%{RELEASE}\\n' 2>/dev/null || true"),
    ("firewall_ufw", "ufw status 2>/dev/null || true"),
    ("firewall_cmd", "firewall-cmd --state 2>/dev/null || true"),
    ("iptables", "iptables -S 2>/dev/null | head -50 || true"),
    ("listening", "ss -tlnH 2>/dev/null || netstat -tln 2>/dev/null || true"),
    ("shadow_perm", "stat -c '%a %U %G' /etc/shadow 2>/dev/null || true"),
    ("world_writable", "find /etc /usr/local/bin -maxdepth 2 -type f -perm -o+w 2>/dev/null | head -20 || true"),
    ("sudoers_nopasswd", "grep -rE 'NOPASSWD' /etc/sudoers /etc/sudoers.d/ 2>/dev/null | head -10 || true"),
    ("pending_updates", "apt-get -s upgrade 2>/dev/null | grep -c '^Inst' || true"),
];

/// Động từ có thể thay đổi hệ thống. Dùng cho test chặn hồi quy.
pub const MUTATING: &[&str] = &[
    "rm ", "rmdir", "mv ", "cp ", "chmod", "chown", "mkdir", "touch", "tee",
    "dd ", "mkfs", "kill", "reboot", "shutdown", "systemctl start", "systemctl stop",
    "apt-get install", "apt install", "yum install", "useradd", "userdel", "passwd",
    "iptables -A", "iptables -D", "ufw allow", "ufw deny", "truncate", "sed -i",
];

// ---------------------------------------------------------------------------
// Phân tích (thuần, test được không cần SSH)
// ---------------------------------------------------------------------------

/// `/etc/os-release` → hệ sinh thái OSV (`Debian:12`, `Ubuntu:22.04:LTS`).
///
/// Sai chuỗi này thì OSV trả rỗng và kết quả trông như "không có lỗ hổng" —
/// cùng loại bẫy với Maven thiếu groupId.
pub fn osv_ecosystem(os_release: &str) -> Option<String> {
    let mut kv = HashMap::new();
    for line in os_release.lines() {
        if let Some((k, v)) = line.split_once('=') {
            kv.insert(k.trim(), v.trim().trim_matches('"').to_string());
        }
    }
    let id = kv.get("ID")?.to_ascii_lowercase();
    let ver = kv.get("VERSION_ID").cloned().unwrap_or_default();
    match id.as_str() {
        "debian" => Some(format!("Debian:{}", ver.split('.').next().unwrap_or(&ver))),
        // Ubuntu LTS mang hậu tố riêng trong OSV; bản không phải LTS thì không.
        "ubuntu" => {
            let lts = kv
                .get("VERSION")
                .map(|v| v.contains("LTS"))
                .unwrap_or(false);
            Some(if lts {
                format!("Ubuntu:{ver}:LTS")
            } else {
                format!("Ubuntu:{ver}")
            })
        }
        "alpine" => Some(format!(
            "Alpine:v{}",
            ver.split('.').take(2).collect::<Vec<_>>().join(".")
        )),
        "rocky" | "almalinux" | "rhel" | "centos" => Some(format!(
            "Rocky Linux:{}",
            ver.split('.').next().unwrap_or(&ver)
        )),
        _ => None,
    }
}

/// `dpkg-query`/`rpm -qa` → danh sách gói.
pub fn parse_packages(out: &str, ecosystem: &str) -> Vec<Package> {
    out.lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let name = it.next()?;
            let ver = it.next()?;
            if name.is_empty() || ver.is_empty() {
                return None;
            }
            Some(Package::new(ecosystem, name, ver))
        })
        .collect()
}

/// `sshd -T` (khoá viết thường) hoặc `sshd_config` thô.
pub fn parse_sshd(out: &str) -> HashMap<String, String> {
    let mut m = HashMap::new();
    for line in out.lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        let mut it = l.splitn(2, char::is_whitespace);
        let (Some(k), Some(v)) = (it.next(), it.next()) else {
            continue;
        };
        // `sshd -T` in ra khoá viết thường; tệp cấu hình giữ nguyên hoa thường.
        m.insert(k.to_ascii_lowercase(), v.trim().to_string());
    }
    m
}

/// `ss -tlnH` → danh sách (địa chỉ, cổng) đang lắng nghe.
pub fn parse_listening(out: &str) -> Vec<(String, u16)> {
    out.lines()
        .filter_map(|l| {
            // ss: State Recv-Q Send-Q Local:Port Peer:Port
            // netstat: Proto Recv-Q Send-Q Local Foreign State
            let f: Vec<&str> = l.split_whitespace().collect();
            let local = f.iter().find(|x| x.contains(':') && x.rsplit(':').next()
                .map(|p| p.chars().all(|c| c.is_ascii_digit()) && !p.is_empty())
                .unwrap_or(false))?;
            let (addr, port) = local.rsplit_once(':')?;
            Some((addr.trim_matches(['[', ']']).to_string(), port.parse().ok()?))
        })
        .collect()
}

/// Cổng này có phơi ra ngoài loopback không.
fn is_exposed(addr: &str) -> bool {
    matches!(addr, "0.0.0.0" | "*" | "::" | "" )
}

/// Dịch vụ không nên phơi thẳng ra Internet.
const SENSITIVE_PORTS: &[(u16, &str, &str)] = &[
    (3306, "MySQL", "critical"),
    (5432, "PostgreSQL", "critical"),
    (27017, "MongoDB", "critical"),
    (6379, "Redis", "critical"),
    (9200, "Elasticsearch", "critical"),
    (11211, "Memcached", "critical"),
    (5984, "CouchDB", "critical"),
    (2375, "Docker API", "critical"),
    (23, "Telnet", "high"),
    (21, "FTP", "medium"),
    (3389, "RDP", "high"),
    (5900, "VNC", "high"),
];

pub struct HostFacts {
    pub raw: HashMap<String, String>,
}

impl HostFacts {
    pub fn get(&self, k: &str) -> &str {
        self.raw.get(k).map(|s| s.as_str()).unwrap_or("")
    }
}

pub fn analyze(f: &HostFacts) -> Vec<Finding> {
    let mut out = vec![];
    let ssh = parse_sshd(f.get("sshd_config"));

    // --- SSH ---
    match ssh.get("permitrootlogin").map(|s| s.as_str()) {
        Some("yes") => out.push(
            Finding::new("ssh", "high", "host:ssh:root-login", "SSH cho phép đăng nhập thẳng bằng root")
                .detail("Mọi lần dò mật khẩu đều biết sẵn tên người dùng, và mọi hành động sau đó mất dấu vết ai thực hiện.")
                .fix("Đặt 'PermitRootLogin no' rồi dùng sudo từ tài khoản riêng.")
                .wstg("WSTG-CONF-05"),
        ),
        Some("yes-password") | Some("without-password") | Some("prohibit-password") => {}
        _ => {}
    }
    if ssh.get("passwordauthentication").map(|s| s == "yes").unwrap_or(false) {
        out.push(
            Finding::new("ssh", "medium", "host:ssh:password-auth", "SSH còn cho xác thực bằng mật khẩu")
                .detail("Mở đường cho dò mật khẩu tự động. Khoá công khai vừa an toàn hơn vừa tiện hơn.")
                .fix("Đặt 'PasswordAuthentication no' SAU KHI đã chắc chắn khoá công khai hoạt động.")
                .wstg("WSTG-CONF-05"),
        );
    }
    if ssh.get("permitemptypasswords").map(|s| s == "yes").unwrap_or(false) {
        out.push(
            Finding::new("ssh", "critical", "host:ssh:empty-passwords", "SSH cho phép mật khẩu RỖNG")
                .detail("Bất kỳ tài khoản nào không đặt mật khẩu đều đăng nhập được từ xa.")
                .fix("Đặt 'PermitEmptyPasswords no' ngay."),
        );
    }

    // --- tường lửa ---
    let ufw = f.get("firewall_ufw").to_ascii_lowercase();
    let fwd = f.get("firewall_cmd").trim();
    let ipt = f.get("iptables");
    let has_fw = ufw.contains("status: active")
        || fwd == "running"
        || ipt.lines().filter(|l| l.starts_with("-A")).count() > 2;
    if !has_fw && !ipt.is_empty() {
        out.push(
            Finding::new("host", "medium", "host:firewall:inactive", "Không thấy tường lửa đang bật")
                .detail("Không có ufw/firewalld đang chạy và bảng iptables gần như trống — mọi cổng đang mở đều tiếp cận được từ mạng.")
                .fix("Bật ufw và chỉ mở đúng cổng cần thiết."),
        );
    }

    // --- cổng phơi ra ngoài ---
    for (addr, port) in parse_listening(f.get("listening")) {
        if !is_exposed(&addr) {
            continue;
        }
        if let Some((_, name, sev)) = SENSITIVE_PORTS.iter().find(|(p, _, _)| *p == port) {
            out.push(
                Finding::new("host", sev, format!("host:port:{port}"), format!("{name} lắng nghe trên mọi giao diện (cổng {port})"))
                    .detail(format!("{name} thường không có xác thực mạnh mặc định. Nghe trên 0.0.0.0 nghĩa là bất kỳ ai tới được máy này đều kết nối thử."))
                    .evidence(json!({ "address": addr, "port": port }))
                    .fix(format!("Bind {name} vào 127.0.0.1, hoặc chặn cổng {port} ở tường lửa.")),
            );
        }
    }

    // --- quyền tệp ---
    let shadow = f.get("shadow_perm");
    if let Some(mode) = shadow.split_whitespace().next() {
        // Chữ số cuối là quyền của 'other'. Khác 0 nghĩa là mọi người đọc được
        // băm mật khẩu.
        if let Some(last) = mode.chars().last().and_then(|c| c.to_digit(8)) {
            if last != 0 {
                out.push(
                    Finding::new("host", "critical", "host:perm:shadow", "/etc/shadow đọc được bởi mọi người dùng")
                        .detail("Băm mật khẩu của toàn bộ tài khoản lộ ra cho bất kỳ ai đăng nhập được vào máy, kể cả tài khoản dịch vụ bị chiếm.")
                        .evidence(json!({ "mode": mode }))
                        .fix("chmod 640 /etc/shadow && chown root:shadow /etc/shadow"),
                );
            }
        }
    }
    let ww: Vec<&str> = f.get("world_writable").lines().filter(|l| !l.trim().is_empty()).collect();
    if !ww.is_empty() {
        out.push(
            Finding::new("host", "high", "host:perm:world-writable", format!("{} tệp hệ thống ai cũng ghi được", ww.len()))
                .detail("Tệp trong /etc hoặc /usr/local/bin mà mọi người dùng ghi được là đường leo thang đặc quyền trực tiếp.")
                .evidence(json!({ "files": ww }))
                .fix("chmod o-w cho từng tệp, rồi tìm xem cái gì đã đặt quyền đó."),
        );
    }

    let nopasswd: Vec<&str> = f
        .get("sudoers_nopasswd")
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .collect();
    if !nopasswd.is_empty() {
        out.push(
            Finding::new("host", "medium", "host:sudo:nopasswd", "Có quy tắc sudo NOPASSWD")
                .detail("Tài khoản bị chiếm sẽ lên root mà không cần biết mật khẩu nào. Đôi khi đây là chủ ý (tự động hoá) — nếu vậy hãy giới hạn đúng lệnh cần thiết.")
                .evidence(json!({ "rules": nopasswd }))
                .fix("Thu hẹp NOPASSWD về đúng những lệnh cụ thể, đừng để ALL."),
        );
    }

    if let Ok(n) = f.get("pending_updates").trim().parse::<u32>() {
        if n > 0 {
            let sev = if n > 50 { "medium" } else { "low" };
            out.push(
                Finding::new("host", sev, "host:updates:pending", format!("{n} gói chờ cập nhật"))
                    .detail("Không phải bản cập nhật nào cũng là bản vá bảo mật, nhưng tồn đọng lớn thường đi kèm CVE đã có bản vá.")
                    .evidence(json!({ "count": n }))
                    .fix("apt-get update && apt-get upgrade (kiểm thử trước khi chạy trên production)."),
            );
        }
    }

    out
}

/// Danh sách gói của máy, kèm hệ sinh thái OSV. Rỗng nếu không nhận diện được OS.
pub fn packages(f: &HostFacts) -> Vec<Package> {
    let Some(eco) = osv_ecosystem(f.get("os_release")) else {
        return vec![];
    };
    let deb = parse_packages(f.get("packages_deb"), &eco);
    if !deb.is_empty() {
        return deb;
    }
    parse_packages(f.get("packages_rpm"), &eco)
}

// ---------------------------------------------------------------------------
// Chạy qua ssh-manager
// ---------------------------------------------------------------------------

/// Địa chỉ app ssh-manager. **Không hardcode cổng** — ssh-manager không đặt
/// `runtime.port` nên daemon cấp cổng động; phải khai qua biến môi trường.
pub fn ssh_manager_url() -> Option<String> {
    std::env::var("SECSCAN_SSH_MANAGER_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim_end_matches('/').to_string())
}

async fn call_ssh_tool(
    http: &reqwest::Client,
    base: &str,
    name: &str,
    args: Value,
) -> Result<String> {
    let body = json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": { "name": name, "arguments": args }
    });
    let v: Value = http
        .post(format!("{base}/api/mcp/message"))
        .json(&body)
        .timeout(std::time::Duration::from_secs(60))
        .send()
        .await
        .map_err(|e| anyhow!("không gọi được ssh-manager ({base}) — app đó có đang chạy không? {e}"))?
        .json()
        .await
        .map_err(|e| anyhow!("ssh-manager trả về không phải JSON: {e}"))?;
    let result = &v["result"];
    let text = result["content"][0]["text"].as_str().unwrap_or("").to_string();
    if result["isError"].as_bool().unwrap_or(false) {
        bail!("{name} lỗi: {}", text.chars().take(300).collect::<String>());
    }
    Ok(text)
}

/// Thu thập dữ liệu từ máy chủ. `ssh_ref` là id máy bên ssh-manager —
/// **secscan không bao giờ thấy mật khẩu hay khoá riêng.**
pub async fn collect(http: &reqwest::Client, ssh_ref: &str) -> Result<HostFacts> {
    let base = ssh_manager_url().ok_or_else(|| {
        anyhow!(
            "chưa biết địa chỉ app ssh-manager. Đặt SECSCAN_SSH_MANAGER_URL (ví dụ http://127.0.0.1:PORT) — \
             ssh-manager dùng cổng động nên không thể đoán."
        )
    })?;

    let conn = call_ssh_tool(http, &base, "ssh_start_connect_id", json!({ "id": ssh_ref })).await?;
    // ssh-manager trả JSON có connection_id; nếu đổi định dạng thì dùng luôn ssh_ref.
    let connection_id = serde_json::from_str::<Value>(&conn)
        .ok()
        .and_then(|v| v["connection_id"].as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| ssh_ref.to_string());

    let mut raw = HashMap::new();
    for (key, cmd) in COMMANDS {
        let out = call_ssh_tool(
            http,
            &base,
            "ssh_execute_command",
            json!({ "connection_id": connection_id, "command": cmd }),
        )
        .await
        .unwrap_or_default();
        raw.insert(key.to_string(), out);
    }
    let _ = call_ssh_tool(http, &base, "ssh_close_connect", json!({ "connection_id": connection_id })).await;

    Ok(HostFacts { raw })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(pairs: &[(&str, &str)]) -> HostFacts {
        HostFacts {
            raw: pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        }
    }
    fn ids(f: &[Finding]) -> Vec<&str> {
        f.iter().map(|x| x.fingerprint.as_str()).collect()
    }

    #[test]
    fn every_command_is_read_only() {
        // Chốt chặn quan trọng nhất của module: L3 chạy lệnh trên máy NGƯỜI KHÁC.
        // Thêm một lệnh có động từ ghi/xoá là test này đỏ.
        //
        // Khớp theo TOKEN (tách theo khoảng trắng và toán tử shell), không phải
        // chuỗi con — bản đầu bắt nhầm 'rm ' trong '-perm ' của find.
        fn tokens(cmd: &str) -> Vec<String> {
            cmd.split(|c: char| c.is_whitespace() || "|;&()<>'\"`".contains(c))
                .filter(|s| !s.is_empty())
                .map(|s| s.to_ascii_lowercase())
                .collect()
        }
        for (key, cmd) in COMMANDS {
            let toks = tokens(cmd);
            for bad in MUTATING {
                let bad_toks: Vec<&str> = bad.split_whitespace().collect();
                let n = bad_toks.len();
                for window in toks.windows(n) {
                    let matches_all = window
                        .iter()
                        .zip(bad_toks.iter())
                        .all(|(a, b)| a == *b);
                    assert!(
                        !matches_all,
                        "lệnh '{key}' chứa động từ thay đổi hệ thống: '{bad}'"
                    );
                }
            }
            // chuyển hướng ghi cũng bị cấm (không tính '2>/dev/null')
            let c = cmd.to_ascii_lowercase();
            let redirects = c.matches('>').count();
            let devnull = c.matches("2>/dev/null").count() + c.matches(">/dev/null").count();
            assert!(
                redirects <= devnull,
                "lệnh '{key}' có chuyển hướng ghi ngoài /dev/null"
            );
        }
    }

    #[test]
    fn os_release_maps_to_the_exact_osv_ecosystem_string() {
        // Sai chuỗi này thì OSV trả rỗng và kết quả trông như "không có lỗ hổng".
        assert_eq!(
            osv_ecosystem("ID=debian\nVERSION_ID=\"12\"\n").as_deref(),
            Some("Debian:12")
        );
        assert_eq!(
            osv_ecosystem("ID=ubuntu\nVERSION_ID=\"22.04\"\nVERSION=\"22.04.3 LTS (Jammy)\"").as_deref(),
            Some("Ubuntu:22.04:LTS")
        );
        // bản không LTS KHÔNG mang hậu tố
        assert_eq!(
            osv_ecosystem("ID=ubuntu\nVERSION_ID=\"23.10\"\nVERSION=\"23.10 (Mantic)\"").as_deref(),
            Some("Ubuntu:23.10")
        );
        assert_eq!(
            osv_ecosystem("ID=alpine\nVERSION_ID=3.19.1").as_deref(),
            Some("Alpine:v3.19")
        );
        assert!(osv_ecosystem("ID=plan9").is_none(), "OS lạ thì thà không tra còn hơn tra sai");
        assert!(osv_ecosystem("").is_none());
    }

    #[test]
    fn parses_package_listings() {
        let deb = "nginx 1.22.1-9\nopenssl 3.0.11-1~deb12u2\nbash 5.2.15-2";
        let p = parse_packages(deb, "Debian:12");
        assert_eq!(p.len(), 3);
        assert_eq!(p[0].name, "nginx");
        assert_eq!(p[0].version, "1.22.1-9");
        assert_eq!(p[0].ecosystem, "Debian:12");
        // dòng rác không làm hỏng cả danh sách
        assert_eq!(parse_packages("hỏng\nnginx 1.0\n\n", "Debian:12").len(), 1);
    }

    #[test]
    fn parses_sshd_output_in_both_formats() {
        // `sshd -T` in khoá viết thường
        let t = parse_sshd("permitrootlogin yes\npasswordauthentication no\n");
        assert_eq!(t["permitrootlogin"], "yes");
        // tệp cấu hình giữ hoa thường + có chú thích
        let c = parse_sshd("# comment\nPermitRootLogin no\n\n  PasswordAuthentication yes  \n");
        assert_eq!(c["permitrootlogin"], "no");
        assert_eq!(c["passwordauthentication"], "yes");
    }

    #[test]
    fn root_login_and_empty_passwords_are_ranked_by_consequence() {
        let f = analyze(&facts(&[(
            "sshd_config",
            "permitrootlogin yes\npasswordauthentication yes\npermitemptypasswords yes",
        )]));
        let by = |id: &str| f.iter().find(|x| x.fingerprint == id).unwrap().severity;
        assert_eq!(by("host:ssh:empty-passwords"), "critical");
        assert_eq!(by("host:ssh:root-login"), "high");
        assert_eq!(by("host:ssh:password-auth"), "medium");
    }

    #[test]
    fn a_hardened_sshd_produces_nothing() {
        let f = analyze(&facts(&[(
            "sshd_config",
            "permitrootlogin no\npasswordauthentication no\npermitemptypasswords no",
        )]));
        assert!(f.is_empty(), "cấu hình tốt không được sinh cảnh báo: {:?}", ids(&f));
    }

    #[test]
    fn prohibit_password_is_not_treated_as_root_login() {
        // 'prohibit-password' là cấu hình ĐÚNG cho tự động hoá bằng khoá.
        let f = analyze(&facts(&[("sshd_config", "permitrootlogin prohibit-password")]));
        assert!(!ids(&f).contains(&"host:ssh:root-login"));
    }

    #[test]
    fn listening_ports_are_parsed_from_both_ss_and_netstat() {
        let ss = "LISTEN 0 511 0.0.0.0:3306 0.0.0.0:*\nLISTEN 0 4096 127.0.0.1:6379 0.0.0.0:*";
        let l = parse_listening(ss);
        assert!(l.contains(&("0.0.0.0".into(), 3306)));
        assert!(l.contains(&("127.0.0.1".into(), 6379)));
    }

    #[test]
    fn only_ports_exposed_beyond_loopback_are_flagged() {
        let f = analyze(&facts(&[(
            "listening",
            "LISTEN 0 511 0.0.0.0:3306 *:*\nLISTEN 0 511 127.0.0.1:5432 *:*\nLISTEN 0 511 0.0.0.0:443 *:*",
        )]));
        // MySQL phơi ra ngoài -> báo; PostgreSQL chỉ loopback -> không;
        // 443 phơi ra ngoài nhưng đó là việc bình thường -> không.
        assert!(ids(&f).contains(&"host:port:3306"));
        assert!(!ids(&f).contains(&"host:port:5432"), "loopback không phải lỗ hổng");
        assert!(!ids(&f).iter().any(|i| i.contains("443")));
    }

    #[test]
    fn world_readable_shadow_is_critical() {
        let f = analyze(&facts(&[("shadow_perm", "644 root root")]));
        let s = f.iter().find(|x| x.fingerprint == "host:perm:shadow").unwrap();
        assert_eq!(s.severity, "critical");
        // quyền đúng thì im lặng
        assert!(analyze(&facts(&[("shadow_perm", "640 root shadow")])).is_empty());
    }

    #[test]
    fn firewall_is_only_flagged_when_we_actually_looked() {
        // Không có dữ liệu iptables = không kiểm được, KHÔNG được kết luận "không có tường lửa".
        assert!(analyze(&facts(&[])).is_empty());
        // có dữ liệu, bảng trống -> báo
        let f = analyze(&facts(&[("iptables", "-P INPUT ACCEPT\n-P FORWARD ACCEPT")]));
        assert!(ids(&f).contains(&"host:firewall:inactive"));
        // ufw đang bật -> không báo
        let g = analyze(&facts(&[
            ("iptables", "-P INPUT ACCEPT"),
            ("firewall_ufw", "Status: active"),
        ]));
        assert!(!ids(&g).contains(&"host:firewall:inactive"));
    }

    #[test]
    fn pending_updates_scale_with_the_backlog() {
        let sev = |n: &str| {
            analyze(&facts(&[("pending_updates", n)]))
                .iter()
                .find(|x| x.fingerprint == "host:updates:pending")
                .map(|x| x.severity)
        };
        assert_eq!(sev("0"), None, "không tồn đọng thì không báo");
        assert_eq!(sev("5"), Some("low"));
        assert_eq!(sev("120"), Some("medium"));
    }

    #[test]
    fn packages_need_a_recognised_os_before_being_queried() {
        // OS lạ -> danh sách rỗng, thà không tra còn hơn tra nhầm hệ sinh thái
        // rồi nhận về "không có lỗ hổng" một cách giả tạo.
        let f = facts(&[("os_release", "ID=plan9"), ("packages_deb", "nginx 1.0")]);
        assert!(packages(&f).is_empty());

        let g = facts(&[("os_release", "ID=debian\nVERSION_ID=\"12\""), ("packages_deb", "nginx 1.22.1-9")]);
        let p = packages(&g);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].ecosystem, "Debian:12");
    }

    #[test]
    fn ssh_manager_url_is_never_hardcoded() {
        // ssh-manager không đặt runtime.port nên daemon cấp cổng động — đoán cổng
        // là sai. Không khai biến môi trường thì phải trả None để lỗi nói rõ lý do.
        std::env::remove_var("SECSCAN_SSH_MANAGER_URL");
        assert!(ssh_manager_url().is_none());
        std::env::set_var("SECSCAN_SSH_MANAGER_URL", "http://127.0.0.1:9999/");
        assert_eq!(ssh_manager_url().as_deref(), Some("http://127.0.0.1:9999"));
        std::env::remove_var("SECSCAN_SSH_MANAGER_URL");
    }
}
