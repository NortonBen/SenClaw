//! Công cụ **build chủ đề** — templates biến các engine dựng sẵn (vàng, thời
//! tiết, xổ số, bóng đá) thành CHỦ ĐỀ có connector tự nạp dữ liệu, thay cho
//! các tab mặc định cũ. Một chủ đề connector có `source = {kind, params}`;
//! engine::sync đọc dữ liệu đã fetch sẵn trong DB (không fetch mới) và append
//! bản ghi (dedup theo khoá ngày/sự kiện).

use serde_json::{json, Value};

/// Danh sách template cho UI/agent. `params` mô tả tham số cần khi tạo.
pub fn templates_json() -> Value {
    json!([
        {
            "key": "gold",
            "name": "Giá vàng & tỷ giá",
            "icon": "🪙",
            "description": "Mỗi ngày một bản ghi: XAU/USD, USD/VND, quy đổi triệu VND/lượng — nạp tự động từ engine giá.",
            "params": [],
            "fields": [
                { "name": "ngày", "kind": "date" },
                { "name": "xau_usd", "kind": "number" },
                { "name": "usd_vnd", "kind": "number" },
                { "name": "trieu_luong", "kind": "number" }
            ]
        },
        {
            "key": "weather",
            "name": "Thời tiết thành phố",
            "icon": "🌦",
            "description": "Mỗi ngày một bản ghi dự báo: nhiệt độ min/max, % mưa — chọn thành phố khi tạo.",
            "params": [{ "name": "city", "label": "Thành phố", "default": "Hà Nội" }],
            "fields": [
                { "name": "ngày", "kind": "date" },
                { "name": "t_max", "kind": "number" },
                { "name": "t_min", "kind": "number" },
                { "name": "mua_prob", "kind": "number" }
            ]
        },
        {
            "key": "lottery",
            "name": "Xổ số miền Bắc",
            "icon": "🎰",
            "description": "Mỗi kỳ quay một bản ghi: giải Đặc biệt + 2 số cuối — nạp từ dataset XSMB.",
            "params": [],
            "fields": [
                { "name": "ngày", "kind": "date" },
                { "name": "dac_biet", "kind": "number" },
                { "name": "duoi_db", "kind": "number" }
            ]
        },
        {
            "key": "football",
            "name": "Bóng đá — kết quả giải",
            "icon": "⚽",
            "description": "Mỗi trận đã đá một bản ghi: tỷ số + kết quả H/D/A — chọn giải khi tạo.",
            "params": [{ "name": "league", "label": "Giải (id TheSportsDB)", "default": "4328" }],
            "fields": [
                { "name": "ngày", "kind": "date" },
                { "name": "tran", "kind": "text" },
                { "name": "ban_nha", "kind": "number" },
                { "name": "ban_khach", "kind": "number" },
                { "name": "ket_qua", "kind": "text" }
            ]
        },
        {
            "key": "blank",
            "name": "Chủ đề trống (tự thiết lập)",
            "icon": "📋",
            "description": "Tự định nghĩa trường dữ liệu, nhập tay hoặc import CSV/JSON.",
            "params": [],
            "fields": [
                { "name": "ngày", "kind": "date" },
                { "name": "giá trị", "kind": "number" }
            ]
        }
    ])
}

/// Resolve one template by key. Returns `(name, description, fields, source)`;
/// `params` fills placeholders (city/league). None for unknown keys.
pub fn instantiate(key: &str, params: &Value) -> Option<(String, String, Value, Value)> {
    let templates = templates_json();
    let t = templates
        .as_array()?
        .iter()
        .find(|t| t["key"] == key)?
        .clone();
    let fields = t["fields"].clone();
    let base_desc = t["description"].as_str().unwrap_or("").to_string();
    match key {
        "weather" => {
            let city = params["city"]
                .as_str()
                .filter(|c| !c.trim().is_empty())
                .unwrap_or("Hà Nội");
            let (name, _, _) = crate::fetch::find_city(city)?;
            Some((
                format!("Thời tiết {name}"),
                base_desc,
                fields,
                json!({ "kind": "weather", "city": name }),
            ))
        }
        "football" => {
            let league = params["league"]
                .as_str()
                .filter(|l| !l.trim().is_empty())
                .unwrap_or("4328");
            let lname = crate::fetch::league_name(league);
            Some((
                format!("Bóng đá {lname}"),
                base_desc,
                fields,
                json!({ "kind": "football", "league": league }),
            ))
        }
        "gold" => Some((
            "Giá vàng & tỷ giá".into(),
            base_desc,
            fields,
            json!({ "kind": "gold" }),
        )),
        "lottery" => Some((
            "Xổ số miền Bắc".into(),
            base_desc,
            fields,
            json!({ "kind": "lottery" }),
        )),
        "blank" => {
            let name = params["name"]
                .as_str()
                .filter(|n| !n.trim().is_empty())
                .unwrap_or("Chủ đề mới");
            Some((
                name.to_string(),
                base_desc,
                fields,
                json!({ "kind": "manual" }),
            ))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_shape() {
        let t = templates_json();
        let arr = t.as_array().unwrap();
        assert_eq!(arr.len(), 5);
        for tpl in arr {
            assert!(tpl["key"].is_string() && tpl["name"].is_string());
            assert!(!tpl["fields"].as_array().unwrap().is_empty());
            // Every template's fields must parse into a valid schema.
            assert!(!crate::topic::parse_fields(&tpl["fields"]).is_empty());
        }
    }

    #[test]
    fn instantiate_with_params() {
        let (name, _, fields, source) =
            instantiate("weather", &serde_json::json!({ "city": "da nang" })).unwrap();
        assert_eq!(name, "Thời tiết Đà Nẵng");
        assert_eq!(source["kind"], "weather");
        assert_eq!(source["city"], "Đà Nẵng");
        assert_eq!(crate::topic::parse_fields(&fields).len(), 4);

        let (fname, _, _, fsource) = instantiate("football", &serde_json::json!({})).unwrap();
        assert!(fname.contains("Ngoại hạng Anh"));
        assert_eq!(fsource["league"], "4328");

        let (gname, _, _, gsource) = instantiate("gold", &serde_json::json!({})).unwrap();
        assert!(gname.contains("vàng"));
        assert_eq!(gsource["kind"], "gold");

        let (bname, _, _, bsource) =
            instantiate("blank", &serde_json::json!({ "name": "Cân nặng" })).unwrap();
        assert_eq!(bname, "Cân nặng");
        assert_eq!(bsource["kind"], "manual");

        assert!(instantiate("nope", &serde_json::json!({})).is_none());
        assert!(instantiate("weather", &serde_json::json!({ "city": "Tokyo" })).is_none());
    }
}
