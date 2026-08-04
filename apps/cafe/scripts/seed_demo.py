#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Seed dữ liệu demo cho app Quán Cafe qua REST API.

Tạo một quán cafe "thật": ~19 nguyên liệu, 14 món (4 nhóm) kèm công thức định
lượng, phiếu nhập từ 5 nhà cung cấp trải ~5 tuần (giá dao động nhẹ → thấy được
bình quân gia quyền), ~300 đơn bán trong 30 ngày (cuối tuần đông hơn, vài đơn
huỷ) và vài đơn sáng nay.

NÊN chạy trên DB SẠCH (xoá cafe.db / SENCLAW_DATA_DIR trước) — nguyên liệu và
món là get-or-create nên chạy lại không lỗi, nhưng phiếu nhập/đơn bán sẽ bị
nhân đôi doanh số.

Usage:
    python3 seed_demo.py [--base http://127.0.0.1:4700] [--days 30]
"""
import argparse
import datetime as dt
import json
import random
import urllib.request

BASE = "http://127.0.0.1:4700"


def call(method, path, body=None):
    url = BASE + path
    data = json.dumps(body).encode("utf-8") if body is not None else None
    req = urllib.request.Request(
        url, data=data, method=method, headers={"Content-Type": "application/json"}
    )
    with urllib.request.urlopen(req, timeout=30) as r:
        out = json.loads(r.read().decode("utf-8"))
    if isinstance(out, dict) and out.get("error"):
        raise RuntimeError(f"{method} {path}: {out['error']}")
    return out


def iso(day_offset):
    return (dt.date.today() + dt.timedelta(days=day_offset)).isoformat()


# ---------------------------------------------------------------- nguyên liệu
# (tên, đơn vị gốc, tồn tối thiểu)
INGREDIENTS = [
    ("Cà phê Robusta rang xay", "g", 1000),
    ("Cà phê Arabica", "g", 300),
    ("Sữa đặc", "ml", 2000),
    ("Sữa tươi thanh trùng", "ml", 3000),
    ("Kem béo thực vật", "ml", 1000),
    ("Đường cát", "g", 2000),
    ("Trà đen", "g", 300),
    ("Trà lài", "g", 200),
    ("Trà ô long", "g", 300),
    ("Bột cacao", "g", 300),
    ("Bột matcha", "g", 200),
    ("Trân châu đen", "g", 800),
    ("Đào ngâm", "g", 1000),
    ("Vải ngâm", "g", 800),
    ("Chanh tươi", "g", 400),
    ("Sả cây", "g", 300),
    ("Cam vàng", "g", 1500),
    ("Ly nhựa + nắp", "cái", 100),
    ("Ống hút giấy", "cái", 150),
]

# ---------------------------------------------------------------- thực đơn
# (tên, nhóm, giá, cách pha, [(nguyên liệu, định lượng theo đơn vị gốc)], weight bán)
MENU = [
    ("Cafe đen đá", "Cà phê", 25000,
     "Cho 20g cà phê vào phin, tráng nước sôi, chế 100ml nước 95°C, chờ nhỏ giọt hết rồi thêm đường, lắc với đá.",
     [("Cà phê Robusta rang xay", 20), ("Đường cát", 8), ("Ly nhựa + nắp", 1), ("Ống hút giấy", 1)], 12),
    ("Cafe sữa đá", "Cà phê", 29000,
     "Pha phin 20g cà phê với 25ml nước sôi lót sữa đặc bên dưới, khuấy đều, thêm đá viên.",
     [("Cà phê Robusta rang xay", 20), ("Sữa đặc", 30), ("Ly nhựa + nắp", 1), ("Ống hút giấy", 1)], 18),
    ("Bạc xỉu", "Cà phê", 32000,
     "60ml sữa đặc + 60ml sữa tươi nóng, rót 15g cà phê pha phin lên trên, thêm đá.",
     [("Cà phê Robusta rang xay", 15), ("Sữa đặc", 60), ("Sữa tươi thanh trùng", 60), ("Ly nhựa + nắp", 1), ("Ống hút giấy", 1)], 8),
    ("Cafe muối", "Cà phê", 35000,
     "Cà phê phin 20g + 30ml sữa đặc, phủ kem béo đánh bông với chút muối, thêm đá.",
     [("Cà phê Robusta rang xay", 20), ("Sữa đặc", 30), ("Kem béo thực vật", 30), ("Ly nhựa + nắp", 1), ("Ống hút giấy", 1)], 6),
    ("Espresso Arabica", "Cà phê", 40000,
     "18g Arabica xay mịn, nén, chiết 36ml espresso trong 25-30 giây.",
     [("Cà phê Arabica", 18), ("Ly nhựa + nắp", 1)], 2),
    ("Cacao sữa đá", "Cà phê", 39000,
     "20g bột cacao hoà 30ml nước nóng, thêm 40ml sữa đặc + 100ml sữa tươi, lắc với đá.",
     [("Bột cacao", 20), ("Sữa đặc", 40), ("Sữa tươi thanh trùng", 100), ("Ly nhựa + nắp", 1), ("Ống hút giấy", 1)], 4),
    ("Trà đào cam sả", "Trà trái cây", 45000,
     "Ủ 8g trà đen 5 phút, thêm 60g đào ngâm + 40g cam vàng + 10g sả đập dập + 15g đường, lắc đều với đá, trang trí lát đào.",
     [("Trà đen", 8), ("Đào ngâm", 60), ("Cam vàng", 40), ("Sả cây", 10), ("Đường cát", 15), ("Ly nhựa + nắp", 1), ("Ống hút giấy", 1)], 14),
    ("Trà vải", "Trà trái cây", 42000,
     "Ủ 8g trà lài 5 phút, thêm 70g vải ngâm + 10g đường, lắc với đá, thả 2 trái vải.",
     [("Trà lài", 8), ("Vải ngâm", 70), ("Đường cát", 10), ("Ly nhựa + nắp", 1), ("Ống hút giấy", 1)], 6),
    ("Trà chanh sả", "Trà trái cây", 30000,
     "Ủ 6g trà đen, thêm 30g chanh tươi vắt + 15g sả + 20g đường, lắc với đá, cắm nhánh sả.",
     [("Trà đen", 6), ("Chanh tươi", 30), ("Sả cây", 15), ("Đường cát", 20), ("Ly nhựa + nắp", 1), ("Ống hút giấy", 1)], 7),
    ("Cam vắt", "Trà trái cây", 45000,
     "Vắt 350g cam vàng tươi, thêm 10g đường, khuấy tan, thêm đá.",
     [("Cam vàng", 350), ("Đường cát", 10), ("Ly nhựa + nắp", 1), ("Ống hút giấy", 1)], 4),
    ("Trà sữa ô long", "Trà sữa", 38000,
     "Ủ 7g ô long 8 phút, thêm 40ml sữa đặc + 30ml kem béo + 15g đường, lắc kỹ với đá.",
     [("Trà ô long", 7), ("Sữa đặc", 40), ("Kem béo thực vật", 30), ("Đường cát", 15), ("Ly nhựa + nắp", 1), ("Ống hút giấy", 1)], 8),
    ("Trà sữa trân châu", "Trà sữa", 45000,
     "Trà sữa ô long chuẩn + 50g trân châu đen luộc mềm ủ đường.",
     [("Trà ô long", 7), ("Sữa đặc", 40), ("Kem béo thực vật", 30), ("Trân châu đen", 50), ("Đường cát", 15), ("Ly nhựa + nắp", 1), ("Ống hút giấy", 1)], 9),
    ("Matcha latte", "Trà sữa", 55000,
     "Đánh tan 5g matcha với 30ml nước 80°C, rót 120ml sữa tươi + 20ml sữa đặc, thêm đá.",
     [("Bột matcha", 5), ("Sữa tươi thanh trùng", 120), ("Sữa đặc", 20), ("Ly nhựa + nắp", 1), ("Ống hút giấy", 1)], 4),
    ("Sữa tươi trân châu đường đen", "Trà sữa", 42000,
     "Tráng thành ly bằng 25g đường nấu caramel, thêm 60g trân châu + 180ml sữa tươi + đá.",
     [("Sữa tươi thanh trùng", 180), ("Trân châu đen", 60), ("Đường cát", 25), ("Ly nhựa + nắp", 1), ("Ống hút giấy", 1)], 6),
]

# ---------------------------------------------------------------- phiếu nhập
# (ngày offset so với hôm nay, NCC, ghi chú, [(nguyên liệu, SL, đơn vị, đơn giá/đơn vị)])
PURCHASES = [
    (-34, "Cà phê Đắk Lắk Nguyên Chất", "nhập kho khai trương",
     [("Cà phê Robusta rang xay", 5, "kg", 185000), ("Cà phê Arabica", 1, "kg", 320000)]),
    (-34, "Đại lý Sữa Thành Phát", "nhập kho khai trương",
     [("Sữa đặc", 8, "l", 44000), ("Sữa tươi thanh trùng", 10, "l", 32000), ("Kem béo thực vật", 2, "l", 52000)]),
    (-34, "Phúc Long Mart", "nguyên liệu pha chế khai trương",
     [("Trà đen", 1, "kg", 200000), ("Trà lài", 0.5, "kg", 280000), ("Trà ô long", 1, "kg", 350000),
      ("Bột cacao", 1, "kg", 240000), ("Bột matcha", 0.3, "kg", 650000),
      ("Trân châu đen", 3, "kg", 58000), ("Đường cát", 5, "kg", 21000)]),
    (-34, "Bao Bì Việt", "",
     [("Ly nhựa + nắp", 600, "cái", 680), ("Ống hút giấy", 600, "cái", 180)]),
    (-34, "Chợ đầu mối Thủ Đức", "trái cây tuần 1",
     [("Đào ngâm", 4, "kg", 92000), ("Vải ngâm", 2, "kg", 88000), ("Chanh tươi", 1, "kg", 34000),
      ("Sả cây", 1, "kg", 38000), ("Cam vàng", 6, "kg", 62000)]),
    (-27, "Chợ đầu mối Thủ Đức", "trái cây tuần 2",
     [("Đào ngâm", 3, "kg", 95000), ("Cam vàng", 5, "kg", 66000), ("Chanh tươi", 1, "kg", 36000),
      ("Sả cây", 1, "kg", 40000), ("Vải ngâm", 1.5, "kg", 90000)]),
    (-24, "Đại lý Sữa Thành Phát", "",
     [("Sữa đặc", 6, "l", 45000), ("Sữa tươi thanh trùng", 8, "l", 33000), ("Kem béo thực vật", 2, "l", 54000)]),
    (-20, "Cà phê Đắk Lắk Nguyên Chất", "",
     [("Cà phê Robusta rang xay", 3, "kg", 190000)]),
    (-20, "Chợ đầu mối Thủ Đức", "trái cây tuần 3",
     [("Đào ngâm", 3, "kg", 96000), ("Cam vàng", 5, "kg", 64000)]),
    (-16, "Bao Bì Việt", "",
     [("Ly nhựa + nắp", 400, "cái", 700), ("Ống hút giấy", 400, "cái", 190)]),
    (-13, "Chợ đầu mối Thủ Đức", "trái cây tuần 4",
     [("Đào ngâm", 2, "kg", 94000), ("Cam vàng", 4, "kg", 65000), ("Chanh tươi", 1, "kg", 35000),
      ("Sả cây", 0.8, "kg", 41000), ("Vải ngâm", 1, "kg", 92000)]),
    (-13, "Phúc Long Mart", "bổ sung trà + topping",
     [("Trân châu đen", 3, "kg", 60000), ("Đường cát", 5, "kg", 22000), ("Trà đen", 1, "kg", 205000),
      ("Trà ô long", 0.8, "kg", 355000), ("Trà lài", 0.5, "kg", 285000)]),
    (-10, "Đại lý Sữa Thành Phát", "",
     [("Sữa đặc", 6, "l", 46000), ("Sữa tươi thanh trùng", 8, "l", 32500), ("Kem béo thực vật", 1.5, "l", 53000)]),
    (-6, "Cà phê Đắk Lắk Nguyên Chất", "",
     [("Cà phê Robusta rang xay", 2, "kg", 192000), ("Cà phê Arabica", 0.5, "kg", 330000)]),
    (-6, "Chợ đầu mối Thủ Đức", "",
     [("Đào ngâm", 2, "kg", 97000), ("Cam vàng", 4, "kg", 68000)]),
    (-3, "Bao Bì Việt", "",
     [("Ly nhựa + nắp", 300, "cái", 720), ("Ống hút giấy", 300, "cái", 200)]),
    (-2, "Chợ đầu mối Thủ Đức", "trái cây cuối tuần",
     [("Đào ngâm", 1.5, "kg", 95000), ("Cam vàng", 3, "kg", 66000), ("Chanh tươi", 0.5, "kg", 36000),
      ("Sả cây", 0.5, "kg", 40000)]),
    (-2, "Đại lý Sữa Thành Phát", "",
     [("Sữa đặc", 4, "l", 46000), ("Sữa tươi thanh trùng", 6, "l", 33000)]),
]

NOTES = ["", "", "", "mang đi", "tại quán", "ship GrabFood", "khách quen", "", "bàn 3", ""]


def main():
    global BASE
    ap = argparse.ArgumentParser()
    ap.add_argument("--base", default=BASE)
    ap.add_argument("--days", type=int, default=30, help="số ngày lịch sử bán")
    args = ap.parse_args()
    BASE = args.base.rstrip("/")
    random.seed(42)

    print(f"==> seed vào {BASE}")

    # Nguyên liệu (get-or-create)
    existing = {i["name"]: i["id"] for i in call("GET", "/api/ingredients?include_inactive=true")["ingredients"]}
    ing_id = {}
    for name, unit, min_stock in INGREDIENTS:
        if name in existing:
            ing_id[name] = existing[name]
        else:
            r = call("POST", "/api/ingredients", {"name": name, "unit": unit, "min_stock": min_stock})
            ing_id[name] = r["ingredient"]["id"]
    print(f"  nguyên liệu: {len(ing_id)}")

    # Món + công thức (get-or-create + replace recipe)
    existing_m = {m["name"]: m["id"] for m in call("GET", "/api/menu?include_inactive=true")["menu"]}
    menu_id, weights = {}, {}
    for name, cat, price, instructions, recipe, w in MENU:
        if name in existing_m:
            mid = existing_m[name]
        else:
            r = call("POST", "/api/menu", {"name": name, "category": cat, "price": price, "instructions": instructions})
            mid = r["menu"]["id"]
        call("POST", f"/api/menu/{mid}/recipe", {"items": [{"ingredient_id": ing_id[n], "qty": q} for n, q in recipe]})
        menu_id[name] = mid
        weights[name] = w
    print(f"  món: {len(menu_id)} (đã đặt công thức)")

    # Dòng thời gian: mỗi ngày nhập hàng (nếu có lịch) rồi mới bán — BQGQ chạy đúng thứ tự.
    purchases_by_day = {}
    for off, supplier, note, lines in PURCHASES:
        purchases_by_day.setdefault(off, []).append((supplier, note, lines))

    names = list(weights.keys())
    wlist = [weights[n] for n in names]
    sale_ids, n_purch, n_sales = [], 0, 0
    for off in range(-args.days - 4, 1):
        for supplier, note, lines in purchases_by_day.get(off, []):
            call("POST", "/api/purchases", {
                "supplier": supplier, "date": iso(off), "note": note,
                "lines": [{"ingredient_id": ing_id[n], "qty": q, "unit": u, "unit_price": p} for n, q, u, p in lines],
            })
            n_purch += 1
        if off < -args.days:
            continue
        weekday = (dt.date.today() + dt.timedelta(days=off)).isoweekday()
        if off == 0:
            n_orders = 5  # sáng nay
        else:
            base = 14 if weekday >= 6 else 9
            n_orders = base + random.randint(-2, 2)
        for _ in range(n_orders):
            n_lines = random.choices([1, 2, 3], weights=[55, 33, 12])[0]
            chosen = {}
            for name in random.choices(names, weights=wlist, k=n_lines):
                chosen[name] = chosen.get(name, 0) + random.choices([1, 2], weights=[80, 20])[0]
            r = call("POST", "/api/sales", {
                "date": iso(off), "note": random.choice(NOTES),
                "lines": [{"menu_id": menu_id[n], "qty": q} for n, q in chosen.items()],
            })
            sale_ids.append(r["sale"]["id"])
            n_sales += 1
    print(f"  phiếu nhập: {n_purch}, đơn bán: {n_sales}")

    # Vài đơn ghi nhầm → huỷ (hoàn kho, loại khỏi báo cáo)
    for sid in random.sample(sale_ids[:-8], 2):
        call("POST", f"/api/sales/{sid}/void", {"reason": "ghi nhầm món, đã huỷ"})
    print("  đã huỷ 2 đơn ghi nhầm")

    # Kiểm kê cuối kỳ: hao hụt nhẹ cho vài nguyên liệu tươi (số lẻ tự nhiên)
    for name, delta, reason in [
        ("Sả cây", -120, "kiểm kê: sả héo bỏ"),
        ("Chanh tươi", -180, "kiểm kê: chanh hỏng"),
        ("Trân châu đen", -250, "kiểm kê: trân châu luộc dư bỏ cuối ngày"),
    ]:
        call("POST", "/api/stock/adjust", {"ingredient_id": ing_id[name], "delta": delta, "reason": reason})
    # ...và vài nguyên liệu đếm thực tế thấp hơn sổ → rơi xuống dưới tồn tối
    # thiểu, để dashboard có cảnh báo và Dự đoán có dòng "cần nhập".
    for name, set_qty, reason in [
        ("Bột matcha", 150, "kiểm kê: đổ hộp mới thấy hụt so với sổ"),
        ("Vải ngâm", 600, "kiểm kê: 1 hũ vải bị phồng nắp, bỏ"),
        ("Ống hút giấy", 120, "kiểm kê: ướt hỏng 1 bó"),
    ]:
        call("POST", "/api/stock/adjust", {"ingredient_id": ing_id[name], "set_qty": set_qty, "reason": reason})
    print("  đã ghi 6 phiếu kiểm kê (3 hao hụt + 3 xuống dưới ngưỡng)")

    d = call("GET", "/api/dashboard")
    print("\n==> XONG. Tổng quan:")
    print(f"  Hôm nay: {d['today']['orders']} đơn, doanh thu {d['today']['revenue']:,.0f} đ, lãi gộp {d['today']['profit']:,.0f} đ")
    print(f"  7 ngày:  {d['last7']['orders']} đơn, doanh thu {d['last7']['revenue']:,.0f} đ")
    print(f"  Giá trị tồn kho: {d['stock_value']:,.0f} đ; sắp hết: {len(d['low_stock'])} nguyên liệu")
    for a in d["alerts"]:
        print(f"  ⚠ {a}")


if __name__ == "__main__":
    main()
