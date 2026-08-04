//! Định vị địa lý — kèm **độ tin**, vì con số ở đây không phải là đo đạc.
//!
//! GeoIP là suy luận từ dữ liệu đăng ký và đo độ trễ, không phải GPS. Các nhà
//! cung cấp tự công bố: đúng ở mức **quốc gia ~95–99%**, nhưng ở mức **thành phố
//! chỉ ~55–80%** và tụt mạnh với IP di động hay IP doanh nghiệp. Với IP anycast
//! của CDN thì toạ độ **không có nghĩa gì cả** — nó chỉ ra PoP mà nhà cung cấp
//! khai, chứ máy chủ thật có thể ở châu lục khác.
//!
//! App vì thế in ra độ tin và lý do, thay vì in một tên thành phố trông như sự
//! thật. Đây không phải sự thận trọng thừa: người đọc thường hành động dựa trên
//! cái tên thành phố đó.

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::net::IpAddr;

#[derive(Debug, Default, Clone)]
pub struct Geo {
    pub country: Option<String>,
    pub country_code: Option<String>,
    pub region: Option<String>,
    pub city: Option<String>,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub timezone: Option<String>,
    pub isp: Option<String>,
    pub source: String,
}

/// Mức tin cho từng trường, và **vì sao**.
#[derive(Debug, Clone)]
pub struct Confidence {
    pub country: &'static str,
    pub city: &'static str,
    pub note: String,
    /// Hai nguồn độc lập có cùng kết luận quốc gia không.
    pub agreement: Option<bool>,
}

/// `ipwho.is` — https, không khoá API.
pub fn parse_ipwhois(v: &Value) -> Option<Geo> {
    if v.get("success").and_then(|x| x.as_bool()) == Some(false) {
        return None;
    }
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).map(|x| x.to_string());
    Some(Geo {
        country: s("country"),
        country_code: s("country_code"),
        region: s("region"),
        city: s("city"),
        lat: v.get("latitude").and_then(|x| x.as_f64()),
        lon: v.get("longitude").and_then(|x| x.as_f64()),
        timezone: v
            .get("timezone")
            .and_then(|t| t.get("id"))
            .and_then(|x| x.as_str())
            .map(|x| x.to_string()),
        isp: v
            .get("connection")
            .and_then(|c| c.get("isp").or_else(|| c.get("org")))
            .and_then(|x| x.as_str())
            .map(|x| x.to_string()),
        source: "ipwho.is".into(),
    })
}

/// `ipapi.co` — dự phòng. Cấu trúc phẳng hơn, `country` là mã hai chữ cái.
pub fn parse_ipapi(v: &Value) -> Option<Geo> {
    if v.get("error").and_then(|x| x.as_bool()) == Some(true) {
        return None;
    }
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).map(|x| x.to_string());
    // Không có tên nước lẫn mã nước thì coi như phản hồi rỗng, đừng dựng Geo trống.
    if s("country_name").is_none() && s("country").is_none() {
        return None;
    }
    Some(Geo {
        country: s("country_name"),
        country_code: s("country"),
        region: s("region"),
        city: s("city"),
        lat: v.get("latitude").and_then(|x| x.as_f64()),
        lon: v.get("longitude").and_then(|x| x.as_f64()),
        timezone: s("timezone"),
        isp: s("org"),
        source: "ipapi.co".into(),
    })
}

/// Xếp độ tin từ những gì đã biết về mạng.
///
/// `anycast` là cờ quan trọng nhất: khi IP thuộc CDN, thành phố không chỉ *kém
/// chính xác* mà **không dùng được** — hai người ở hai châu lục truy vấn cùng IP
/// đó sẽ chạm hai máy chủ khác nhau.
pub fn rate(g: Option<&Geo>, other_country: Option<&str>, anycast: bool) -> Confidence {
    let Some(g) = g else {
        return Confidence {
            country: "không có",
            city: "không có",
            note: "Không nguồn nào trả lời — đây là lỗi tra cứu, KHÔNG phải kết luận \
                   'không xác định được vị trí'."
                .into(),
            agreement: None,
        };
    };

    let agreement = match (g.country_code.as_deref(), other_country) {
        (Some(a), Some(b)) if !b.is_empty() => Some(a.eq_ignore_ascii_case(b)),
        _ => None,
    };

    if anycast {
        return Confidence {
            country: "thấp",
            city: "không dùng được",
            note: "IP thuộc mạng CDN/anycast. Cùng một IP được quảng bá ở nhiều nơi \
                   trên thế giới, nên toạ độ chỉ mô tả một PoP — máy chủ gốc có thể ở \
                   châu lục khác. Đừng dùng con số này để suy ra vị trí hạ tầng."
                .into(),
            agreement,
        };
    }

    match agreement {
        Some(false) => Confidence {
            country: "trung bình",
            city: "thấp",
            note: format!(
                "Hai nguồn KHÔNG khớp về quốc gia ({} vs {}). Thường gặp khi IP vừa đổi \
                 chủ hoặc đi qua VPN/proxy — CSDL cập nhật lệch nhau vài tuần.",
                g.country_code.as_deref().unwrap_or("?"),
                other_country.unwrap_or("?")
            ),
            agreement,
        },
        Some(true) => Confidence {
            country: "cao",
            city: "trung bình",
            note: "Hai nguồn độc lập cùng kết luận quốc gia. Mức quốc gia đáng tin \
                   (~95–99%); mức thành phố thì không — các nhà cung cấp tự công bố \
                   chỉ đúng ~55–80% và thấp hơn nữa với IP doanh nghiệp."
                .into(),
            agreement,
        },
        None => Confidence {
            country: "trung bình",
            city: "thấp",
            note: "Chỉ một nguồn trả lời nên không đối chiếu chéo được. Mức quốc gia \
                   thường đúng; mức thành phố nên coi là gợi ý."
                .into(),
            agreement,
        },
    }
}

async fn fetch(http: &reqwest::Client, url: &str) -> Result<Value> {
    let resp = http
        .get(url)
        .send()
        .await
        .map_err(|e| anyhow!("{url} không phản hồi: {e}"))?;
    if !resp.status().is_success() {
        return Err(anyhow!("{url} trả {}", resp.status().as_u16()));
    }
    resp.json()
        .await
        .map_err(|e| anyhow!("{url} trả dữ liệu không đọc được: {e}"))
}

/// Tra hai nguồn song song. Nguồn thứ hai không phải để dự phòng mà để **đối
/// chiếu** — hai CSDL độc lập cùng nói một quốc gia là bằng chứng mạnh hơn hẳn
/// một nguồn nói chắc nịch.
pub async fn locate(http: &reqwest::Client, ip: IpAddr) -> (Option<Geo>, Option<String>) {
    let (u1, u2) = (
        format!("https://ipwho.is/{ip}"),
        format!("https://ipapi.co/{ip}/json/"),
    );
    let (a, b) = tokio::join!(fetch(http, &u1), fetch(http, &u2));
    let primary = a.ok().and_then(|v| parse_ipwhois(&v));
    let secondary = b.ok().and_then(|v| parse_ipapi(&v));

    match (primary, secondary) {
        (Some(p), Some(s)) => {
            let cc = s.country_code.clone();
            (Some(p), cc)
        }
        (Some(p), None) => (Some(p), None),
        // Nguồn chính hỏng thì nguồn phụ lên thay — nhưng khi đó không còn đối
        // chiếu chéo, và `rate()` sẽ hạ độ tin xuống đúng mức đó.
        (None, Some(s)) => (Some(s), None),
        (None, None) => (None, None),
    }
}

pub fn to_json(g: Option<&Geo>, c: &Confidence) -> Value {
    json!({
        "country": g.and_then(|x| x.country.clone()),
        "country_code": g.and_then(|x| x.country_code.clone()),
        "region": g.and_then(|x| x.region.clone()),
        "city": g.and_then(|x| x.city.clone()),
        "lat": g.and_then(|x| x.lat),
        "lon": g.and_then(|x| x.lon),
        "timezone": g.and_then(|x| x.timezone.clone()),
        "isp": g.and_then(|x| x.isp.clone()),
        "source": g.map(|x| x.source.clone()),
        "confidence": {
            "country": c.country,
            "city": c.city,
            "sources_agree": c.agreement,
            "note": c.note,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn geo(cc: &str) -> Geo {
        Geo {
            country_code: Some(cc.into()),
            city: Some("Somewhere".into()),
            ..Default::default()
        }
    }

    #[test]
    fn ipwhois_shape_is_parsed_including_nested_timezone_and_isp() {
        let v = json!({
            "ip": "1.1.1.1", "success": true,
            "country": "Australia", "country_code": "AU",
            "region": "Queensland", "city": "Brisbane",
            "latitude": -27.4766, "longitude": 153.0166,
            "timezone": { "id": "Australia/Brisbane", "abbr": "AEST" },
            "connection": { "asn": 13335, "isp": "Cloudflare, Inc." }
        });
        let g = parse_ipwhois(&v).unwrap();
        assert_eq!(g.city.as_deref(), Some("Brisbane"));
        assert_eq!(g.timezone.as_deref(), Some("Australia/Brisbane"));
        assert_eq!(g.isp.as_deref(), Some("Cloudflare, Inc."));
        assert_eq!(g.lat, Some(-27.4766));
    }

    #[test]
    fn a_failed_lookup_is_not_a_geo_result() {
        assert!(parse_ipwhois(&json!({ "success": false, "message": "reserved" })).is_none());
        assert!(parse_ipapi(&json!({ "error": true, "reason": "RateLimited" })).is_none());
        // phản hồi rỗng cũng không được thành Geo trống
        assert!(parse_ipapi(&json!({})).is_none());
    }

    #[test]
    fn ipapi_flat_shape_is_parsed() {
        let v = json!({
            "ip": "1.1.1.1", "city": "Sydney", "region": "New South Wales",
            "country": "AU", "country_name": "Australia",
            "latitude": -33.8672, "longitude": 151.1997,
            "timezone": "Australia/Sydney", "org": "CLOUDFLARENET"
        });
        let g = parse_ipapi(&v).unwrap();
        assert_eq!(g.country_code.as_deref(), Some("AU"));
        assert_eq!(g.country.as_deref(), Some("Australia"));
        assert_eq!(g.timezone.as_deref(), Some("Australia/Sydney"));
    }

    #[test]
    fn anycast_makes_the_city_unusable_no_matter_how_the_sources_agree() {
        // Đây là điểm quan trọng nhất của module: hai nguồn cùng nói "Brisbane"
        // vẫn không làm cho toạ độ của một IP anycast trở nên có nghĩa.
        let c = rate(Some(&geo("AU")), Some("AU"), true);
        assert_eq!(c.city, "không dùng được");
        assert_eq!(c.country, "thấp");
        assert!(c.note.contains("anycast") || c.note.contains("CDN"));
    }

    #[test]
    fn agreeing_sources_raise_country_confidence() {
        let c = rate(Some(&geo("VN")), Some("vn"), false);
        assert_eq!(c.country, "cao");
        assert_eq!(c.agreement, Some(true));
        // ngay cả khi khớp, thành phố vẫn không được lên "cao"
        assert_ne!(c.city, "cao");
    }

    #[test]
    fn disagreeing_sources_are_reported_not_silently_resolved() {
        let c = rate(Some(&geo("VN")), Some("SG"), false);
        assert_eq!(c.agreement, Some(false));
        assert!(c.note.contains("KHÔNG khớp"));
        assert!(c.note.contains("VN") && c.note.contains("SG"));
    }

    #[test]
    fn a_single_source_cannot_claim_high_confidence() {
        let c = rate(Some(&geo("VN")), None, false);
        assert_eq!(c.country, "trung bình");
        assert!(c.agreement.is_none());
    }

    #[test]
    fn no_data_says_lookup_failed_rather_than_location_unknown() {
        // Cùng nguyên tắc với secscan/dns.rs: "tra không được" ≠ "không có".
        let c = rate(None, None, false);
        assert_eq!(c.country, "không có");
        assert!(c.note.contains("lỗi tra cứu"));
    }
}
