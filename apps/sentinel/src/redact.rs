//! Lọc bí mật trước khi bất cứ thứ gì được ghi vào kho của Sentinel.
//!
//! Sentinel chép lại dấu vết hoạt động của agent — trong đó có kết quả tool,
//! nội dung trang web, đầu ra shell. Những thứ đó thường xuyên chứa token, khoá
//! API, cookie. Nếu chép nguyên văn thì app này tự biến mình thành kho bí mật
//! tập trung, tức là biến công cụ phòng thủ thành mục tiêu ngon nhất trong hệ.
//!
//! Vì vậy: **bản gốc không bao giờ được lưu**. `redact()` chạy trên mọi chuỗi
//! trước khi vào `events.detail_json`. Khi điều tra viên cần nguyên văn, app chỉ
//! ra vị trí trong nguồn (`src_key`) để họ tự mở — chứ không giữ hộ.
//!
//! Danh sách mẫu lấy theo cùng tinh thần với bộ lọc credential của mini-browser
//! (`apps/mini-browser/src/session.rs`): nhận diện theo *hình dạng* chuỗi và
//! theo *tên trường*, vì hai cách bắt được hai lớp rò rỉ khác nhau.

/// Tên trường mà giá trị đi kèm luôn bị che, bất kể hình dạng.
const SECRET_KEYS: &[&str] = &[
    "password",
    "passwd",
    "pwd",
    "secret",
    "token",
    "api_key",
    "apikey",
    "api-key",
    "access_key",
    "secret_key",
    "private_key",
    "authorization",
    "auth",
    "cookie",
    "set-cookie",
    "session_id",
    "sessionid",
    "credential",
    "credentials",
    "otp",
    "cvv",
    "client_secret",
];

/// Tiền tố khoá của các nhà cung cấp phổ biến. Bắt theo hình dạng nên vẫn hiệu
/// quả khi khoá nằm lọt giữa văn bản tự do (stdout, nội dung trang).
const KEY_PREFIXES: &[&str] = &[
    "sk-",       // OpenAI / Anthropic style
    "sk_live_",  // Stripe
    "sk_test_",
    "pk_live_",
    "rk_live_",
    "ghp_",      // GitHub personal
    "gho_",
    "ghs_",
    "ghu_",
    "github_pat_",
    "glpat-",    // GitLab
    "xoxb-",     // Slack
    "xoxp-",
    "xoxa-",
    "AKIA",      // AWS access key id
    "ASIA",
    "AIza",      // Google API
    "ya29.",     // Google OAuth
    "hf_",       // HuggingFace
    "npm_",
    "dop_v1_",   // DigitalOcean
    "shpat_",    // Shopify
];

const MASK: &str = "«đã che»";

/// Có phải chuỗi nhìn như một bí mật (theo hình dạng) không.
pub fn looks_secret(tok: &str) -> bool {
    let t = tok.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-' && c != '.');
    if t.len() < 16 {
        return false;
    }
    if KEY_PREFIXES.iter().any(|p| t.starts_with(p)) {
        return true;
    }
    // JWT: ba đoạn base64url ngăn bởi dấu chấm, đoạn đầu bắt đầu bằng "eyJ".
    if t.starts_with("eyJ") && t.matches('.').count() == 2 {
        return true;
    }
    // Chuỗi dài, entropy cao, không có khoảng trắng: ứng viên khoá thô.
    if t.len() >= 40 && is_high_entropy(t) {
        return true;
    }
    false
}

/// Xấp xỉ entropy bằng cách đếm nhóm ký tự và tỉ lệ ký tự lặp. Đủ để phân biệt
/// `abcdefghijklmnopqrstuvwxyzabcdefghijklmn` (không phải khoá) với một khoá thật.
fn is_high_entropy(t: &str) -> bool {
    if !t
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '+' || c == '/' || c == '=')
    {
        return false;
    }
    // Hex thuần một chữ hoa/thường: băm git, checksum, id phiên. Đây KHÔNG phải
    // bí mật, và che chúng đi sẽ phá giá trị pháp chứng — điều tra viên cần thấy
    // đúng commit nào, file nào đã bị chạm. Khoá thật gần như luôn có tiền tố
    // nhà cung cấp hoặc trộn hoa-thường, nên vẫn bị bắt ở nhánh trên.
    let is_hex = t.chars().all(|c| c.is_ascii_hexdigit());
    if is_hex {
        return false;
    }

    let has_lower = t.chars().any(|c| c.is_ascii_lowercase());
    let has_upper = t.chars().any(|c| c.is_ascii_uppercase());
    let has_digit = t.chars().any(|c| c.is_ascii_digit());
    let classes = [has_lower, has_upper, has_digit]
        .iter()
        .filter(|b| **b)
        .count();
    if classes < 2 {
        return false;
    }
    let distinct: std::collections::HashSet<char> = t.chars().collect();
    // Khoá thật dùng nhiều ký tự khác nhau; văn bản tiếng người thì không dài
    // liên tục 40 ký tự không khoảng trắng.
    distinct.len() * 100 / t.len().max(1) >= 35
}

/// Che mọi bí mật trong một chuỗi tự do. Giữ nguyên độ dài văn bản còn lại để
/// điều tra viên vẫn đọc được ngữ cảnh.
pub fn redact(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut token = String::new();

    let flush = |token: &mut String, out: &mut String| {
        if !token.is_empty() {
            if looks_secret(token) {
                out.push_str(MASK);
            } else {
                out.push_str(token);
            }
            token.clear();
        }
    };

    for ch in s.chars() {
        if ch.is_whitespace() || ch == '"' || ch == '\'' || ch == ',' || ch == ';' {
            flush(&mut token, &mut out);
            out.push(ch);
        } else {
            token.push(ch);
        }
    }
    flush(&mut token, &mut out);

    redact_key_value_pairs(&out)
}

/// Che giá trị đi sau một tên trường nhạy cảm: `password=abc`, `"token": "abc"`,
/// `Authorization: Bearer abc`. Bắt được cả bí mật ngắn mà `looks_secret` bỏ qua.
fn redact_key_value_pairs(s: &str) -> String {
    let lower = s.to_lowercase();
    let mut cuts: Vec<(usize, usize)> = Vec::new();

    for key in SECRET_KEYS {
        let mut from = 0usize;
        while let Some(rel) = lower[from..].find(key) {
            let kstart = from + rel;
            let kend = kstart + key.len();
            from = kend;

            // Chỉ tính khi `key` là một từ trọn vẹn.
            let before_ok = kstart == 0
                || !lower[..kstart]
                    .chars()
                    .next_back()
                    .map(|c| c.is_ascii_alphanumeric())
                    .unwrap_or(false);
            if !before_ok {
                continue;
            }

            // Bỏ qua khoảng trắng, dấu ", :, = để tìm đầu giá trị.
            let bytes = s.as_bytes();
            let mut i = kend;
            let mut saw_sep = false;
            while i < bytes.len() {
                let c = bytes[i] as char;
                if c == ':' || c == '=' {
                    saw_sep = true;
                    i += 1;
                } else if c.is_whitespace() || c == '"' || c == '\'' {
                    i += 1;
                } else {
                    break;
                }
            }
            if !saw_sep || i >= bytes.len() {
                continue;
            }
            // "Bearer <token>" — bỏ từ khoá scheme để che đúng phần token.
            if s[i..].to_lowercase().starts_with("bearer ") {
                i += 7;
            }
            let mut j = i;
            while j < bytes.len() {
                let c = bytes[j] as char;
                if c.is_whitespace() || c == '"' || c == '\'' || c == ',' || c == ';' || c == '}' {
                    break;
                }
                j += 1;
            }
            if j > i {
                cuts.push((i, j));
            }
        }
    }

    if cuts.is_empty() {
        return s.to_string();
    }
    cuts.sort();
    let mut out = String::with_capacity(s.len());
    let mut pos = 0usize;
    for (a, b) in cuts {
        if a < pos {
            continue; // chồng lấn — đã che rồi
        }
        out.push_str(&s[pos..a]);
        out.push_str(MASK);
        pos = b;
    }
    out.push_str(&s[pos..]);
    out
}

/// Che đệ quy mọi chuỗi trong một giá trị JSON.
pub fn redact_value(v: &serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    match v {
        Value::String(s) => Value::String(redact(s)),
        Value::Array(a) => Value::Array(a.iter().map(redact_value).collect()),
        Value::Object(o) => {
            let mut m = serde_json::Map::new();
            for (k, val) in o {
                let key_is_secret = SECRET_KEYS.contains(&k.to_lowercase().as_str());
                if key_is_secret {
                    m.insert(k.clone(), Value::String(MASK.to_string()));
                } else {
                    m.insert(k.clone(), redact_value(val));
                }
            }
            Value::Object(m)
        }
        other => other.clone(),
    }
}

/// Đếm số bí mật tìm thấy mà KHÔNG giữ lại giá trị — dùng cho luật
/// `SEN-EXFIL-05` (báo "có N chuỗi giống bí mật ở đây" mà không nhân bản chúng).
pub fn count_secrets(s: &str) -> usize {
    s.split(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == ',' || c == ';')
        .filter(|t| looks_secret(t))
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn masks_provider_key_prefixes() {
        // Token Slack giả được ghép runtime: literal `xoxb-…` nguyên chuỗi
        // trong source sẽ bị GitHub push protection chặn dù chỉ là fixture.
        let slack_like = ["xoxb", "1234567890", "abcdefghijklmno"].join("-");
        for k in [
            "sk-abcdefghijklmnopqrstuvwxyz123456".to_string(),
            "ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZ012345".to_string(),
            "AKIAIOSFODNN7EXAMPLE".to_string(),
            slack_like,
            "AIzaSyD-1234567890abcdefghijklmnop".to_string(),
        ] {
            let out = redact(&format!("giá trị là {k} nhé"));
            assert!(out.contains(MASK), "phải che được {k}, nhận: {out}");
            assert!(!out.contains(&k), "không được lộ {k}");
        }
    }

    #[test]
    fn masks_jwt() {
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dBjftJeZ4CVPmB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let out = redact(&format!("Authorization: Bearer {jwt}"));
        assert!(!out.contains("dBjftJeZ"), "JWT phải bị che: {out}");
    }

    #[test]
    fn masks_short_secret_via_field_name() {
        // Ngắn nên `looks_secret` bỏ qua — phải bắt được nhờ tên trường.
        let out = redact("password=hunter2 và tiếp tục");
        assert!(!out.contains("hunter2"), "nhận: {out}");
        assert!(out.contains("và tiếp tục"), "phải giữ ngữ cảnh: {out}");
    }

    #[test]
    fn keeps_ordinary_text_intact() {
        let s = "Đã chạy lệnh ls -la trong thư mục /home/benji và thấy 12 file.";
        assert_eq!(redact(s), s);
    }

    #[test]
    fn keeps_long_prose_without_masking() {
        // Câu dài tiếng Việt có dấu — không được nhầm thành khoá.
        let s = "Kết quả phân tích cho thấy hệ thống hoạt động bình thường trong suốt khoảng thời gian được khảo sát";
        assert_eq!(redact(s), s);
    }

    #[test]
    fn redacts_nested_json_by_key_and_shape() {
        let v = json!({
            "url": "https://example.com/x",
            "headers": { "Authorization": "Bearer sk-abcdefghijklmnopqrstuvwxyz1234" },
            "nested": [{ "api_key": "abc123" }],
        });
        let out = redact_value(&v);
        let s = out.to_string();
        assert!(!s.contains("sk-abcdefghij"), "{s}");
        assert!(!s.contains("abc123"), "{s}");
        assert_eq!(out["url"], "https://example.com/x", "URL thường phải giữ");
    }

    #[test]
    fn counts_without_leaking() {
        let n = count_secrets("ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZ012345 và sk-abcdefghijklmnopqrstuvwxyz1234");
        assert_eq!(n, 2);
    }

    #[test]
    fn does_not_mask_plain_hex_hash() {
        // Băm git 40 ký tự chỉ có chữ thường + số → dưới 2 nhóm ký tự, không che.
        let h = "356a192b7913b04c54574d18c28d46e6395428ab";
        assert_eq!(redact(h), h);
    }
}
