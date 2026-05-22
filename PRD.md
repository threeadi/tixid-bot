# PRD — TIX.ID Movie Ticket Bot (Rust)

## 1. Overview

Bot otomatis untuk membeli tiket bioskop di [tix.id](https://tix.id) menggunakan Rust.  
Fokus wilayah: **Kota Malang** (`city_id: 973818515335155712`).  
Target: secepat mungkin dari login → seat hold (order created).

---

## 2. Tech Stack

| Komponen           | Library                     |
| ------------------ | --------------------------- |
| HTTP client        | `reqwest` 0.12 (rustls-tls) |
| Async runtime      | `tokio`                     |
| JSON (de)serialize | `serde` + `serde_json`      |
| Config file        | `toml`                      |
| Error handling     | `anyhow`                    |
| UUID (request_id)  | `uuid` v4                   |
| Time display       | `chrono`                    |

---

## 3. Konfigurasi (`config.toml`)

```toml
[auth]
msisdn   = "+628xxxxxxxxxx"
password = "<RSA-encrypted-password>"   # salin dari browser DevTools

[target]
movie_id = "2039608798242488320"        # ID dari URL tix.id
city_id  = "973818515335155712"         # Malang
date     = ""                           # kosong = auto (tanggal pertama tersedia)

[theater]
# Urutan prioritas bioskop (nama substring, case-insensitive).
# Bot mencoba dari index pertama; jika tidak ada jadwal valid → coba berikutnya.
# Kosongkan [] untuk tidak ada preferensi (pakai bioskop pertama yang tersedia).
theater_priority = [
    "TRANSMART MX MALL XXI",
    "MALANG TOWN SQUARE CINEPOLIS",
    "ARAYA XXI",
    "MALANG CITY POINT CGV",
]

[showtime]
preferred_time_start = "12:00"
preferred_time_end   = "22:00"

[seat]
quantity           = 2
manual_seats       = []                 # ["D6","D7"] → dicoba duluan
avoid_first_rows   = 3                  # skip baris A,B,C
preferred_rows     = ["D", "H"]         # range baris disukai (inklusif)

[device]
device_id = "019e4e4e-f638-7fca-b747-69e48a9eef32"
```

---

## 4. Alur Bot

```
1. Load config.toml
2. POST /v1/auth → guest token (anonymous, client_id="tixid_guest")
3. LOGIN (pakai guest token) → user JWT token
4. GET movie by movie_id → dapat schedule_id (data.id)
5. GET schedule dates → pilih date (config / otomatis = tanggal pertama tersedia)
6. GET theaters + showtimes (page=1) → urutkan berdasarkan theater_priority, filter by showtime preference
7. GET seat layout → status 1 = available, status 6 = blocked
8. SELECT seats (algoritma di bawah)
9. POST create order → tampilkan ringkasan + deadline bayar
10. [Fase 2] Tampilkan QRIS untuk pembayaran
```

---

## 5. Endpoint API

**Base URL semua endpoint:** `https://api-b2b.tix.id`

| Method | Endpoint                                                                   | Keterangan                                                     |
| ------ | -------------------------------------------------------------------------- | -------------------------------------------------------------- |
| POST   | `/v1/auth`                                                                 | Guest token (tanpa auth), body: `{"client_id":"tixid_guest","auth_code":null}` |
| POST   | `/v1/users/login`                                                          | Login, dapat JWT — pakai guest token sebagai Authorization     |
| GET    | `/v1/users`                                                                | Data profil user                                               |
| GET    | `/v1/movies/{movie_id}`                                                    | Detail film + dapat `data.id` (schedule_id)                    |
| GET    | `/v1/schedules/date?schedule_id={id}&city_id={id}`                         | Daftar tanggal tersedia                                        |
| GET    | `/v1/schedules/movies/{schedule_id}?city_id={id}&date={YYYY-MM-DD}&page=1` | Bioskop + jadwal per tanggal                                   |
| GET    | `/v1/movies/{merchant_slug}/layout?show_time_id={id}&tz=7`                 | Peta kursi (XXI → `xxi`, CGV → `cgv`, Cinépolis → `cinepolis`) |
| POST   | `/v1/orders`                                                               | Buat pesanan (seat hold)                                       |
| GET    | `/v1/orders/{order_id}`                                                    | Detail pesanan                                                 |

### Headers wajib setiap request

```
Authorization:   Bearer {token}
app_version:     1.0.0
device_id:       {device_id}
lang:            en
platform:        web
origin:          https://app.tix.id
session_id:      null
user-agent:      Mozilla/5.0 (Windows NT 10.0; Win64; x64) ...Chrome/148...
```

---

## 5a. Contoh Lengkap Request & Response per Endpoint

---

### `POST /v1/users/login`

**Request body:**
```json
{
    "msisdn": "+6289696508086",
    "password": "Ms3PeAYeTa2vpvehHC3euZ9FKMl6tn1Z8Ip5BdCeoj1OoWrEqzN0evtuVWokdtCCWlOS54kbn9apnUUgcTQkOxN+rtv4aQ1JDvsDSVgBOYbiqy0BiMTjCqqhv1qI9ubxuc/YMaigSvsb3BpwLAuINphgPazTQfVbjSE2OCgKGA38R5817/SPgm6/3wvvtAUERQ12su719MALRdJfIL/3bl1DAe0dSfnrldJMjJv/Fx9QVxdq1rAtkM8WmW8uGV0+2I/MtJN8NmkKx+d0RxvBb74w07jcO5xj7a/6tamnRbWCVM0LibqmugwzlPRda1QW355oiZFrpjs6gqkmmCcmLQ=="
}
```

> ⚠️ Password di-encrypt RSA sebelum dikirim. Salin nilai `password` langsung dari
> browser Network tab saat login manual, lalu paste ke `config.toml`.

**Response `200 OK`:**
```json
{
    "success": true,
    "data": {
        "id": "bec54b7b-7515-4b51-a2f9-259f2fd1c9d8",
        "name": "Tri Adi",
        "phone": "+6289696508086",
        "total_active_ticket": 0,
        "total_purchased_ticket": 2,
        "updated_at": 1754841258864,
        "token": "<JWT access token>",
        "refresh_token": "<JWT refresh token>",
        "redirect_url": "https://app.tix.id?token=<JWT access token>"
    }
}
```

> 📌 Simpan `data.token` sebagai Bearer token untuk semua request selanjutnya.

---

### `GET /v1/users`

**Response `200 OK`:**
```json
{
    "success": true,
    "data": {
        "id": "bec54b7b-7515-4b51-a2f9-259f2fd1c9d8",
        "name": "Tri Adi",
        "phone": "+6289696508086",
        "total_active_ticket": 0,
        "total_purchased_ticket": 2,
        "loyaltix_status": "active",
        "updated_at": 1754841258864,
        "email": "triadilaksamana@gmail.com"
    }
}
```

---

### `GET /v1/movies/{movie_id}`

Contoh: `GET /v1/movies/2039608798242488320`

**Response `200 OK`:**
```json
{
    "success": true,
    "data": {
        "id": "2039608799848906752",
        "movie_id": "2039608798242488320",
        "name": "SEMUA AKAN BAIK-BAIK SAJA",
        "status": "NOW_PLAYING",
        "release_date": 1775508600,
        "duration": 113,
        "genres": [{ "id": "1581", "name": "Drama" }],
        "age_category": "R",
        "director": "Baim Wong",
        "actor": "Reza Rahadian, Christine Hakim, ...",
        "synopsis": "...",
        "poster_path": "https://asset.tix.id/movie_poster_v2/4651e5c5-6ddf-4282-a1ce-8304b5e7fc11.webp",
        "rating_score": {
            "vote_average": 9.2,
            "vote_count": 1972
        },
        "country": "Indonesia",
        "trailer": {
            "type": "youtube",
            "key": "M4_0sOryPkU",
            "path": "https://youtu.be/M4_0sOryPkU"
        }
    }
}
```

> 📌 `data.id` = **schedule_id** yang dipakai di endpoint schedules selanjutnya (beda dari `data.movie_id`).

---

### `GET /v1/schedules/date?schedule_id={schedule_id}&city_id={city_id}`

Contoh: `GET /v1/schedules/date?schedule_id=2039608799848906752&city_id=973818515335155712`

**Response `200 OK`:**
```json
{
    "success": true,
    "data": [
        { "date": "2026-05-22", "is_any_schedule": true },
        { "date": "2026-05-23", "is_any_schedule": true },
        { "date": "2026-05-24", "is_any_schedule": true },
        { "date": "2026-05-25", "is_any_schedule": false }
    ]
}
```

> 📌 Filter `is_any_schedule: true` → ambil tanggal pertama jika `config.date` kosong.

---

### `GET /v1/schedules/movies/{schedule_id}?city_id={city_id}&date={YYYY-MM-DD}&page=1`

Contoh: `GET /v1/schedules/movies/2039608799848906752?city_id=973818515335155712&date=2026-05-22&page=1`

**Response `200 OK`:**
```json
{
    "success": true,
    "data": {
        "has_next": false,
        "page": 1,
        "show_date": 1779408000000,
        "theaters": [
            {
                "id": "986744938815295488",
                "name": "ARAYA XXI",
                "type": 0,
                "presale_flag": 0,
                "merchant": {
                    "merchant_id": "2224f7e3-da00-4fb9-9de3-2b888d83ac02",
                    "merchant_name": "XXI"
                },
                "address": "PLAZA ARAYA LT. 2, JL. BLIMBING INDAH MEGAH NO. 2",
                "location": {
                    "latitude": "-7.936532974243164",
                    "longitude": "112.65030670166016"
                },
                "price_groups": [
                    {
                        "category": "2D",
                        "low_price": 40000,
                        "high_price": 40000,
                        "price_string": "Rp40.000",
                        "show_time": [
                            {
                                "id": "2057538032336191488",
                                "time": 1779453900000,
                                "display_time": "12:45",
                                "studio": "2",
                                "expired": 1779452700000,
                                "status": 0,
                                "studio_type": "",
                                "price": 40000
                            },
                            {
                                "id": "2057538032403300352",
                                "time": 1779460200000,
                                "display_time": "14:30",
                                "studio": "5",
                                "expired": 1779459000000,
                                "status": 1,
                                "studio_type": "",
                                "price": 40000
                            }
                        ]
                    }
                ]
            },
            {
                "id": "1542422688763564032",
                "name": "MALANG CITY POINT CGV",
                "type": 0,
                "presale_flag": 0,
                "merchant": {
                    "merchant_id": "2224f7e3-da00-4fb9-9de3-2b888d83ac03",
                    "merchant_name": "CGV"
                },
                "address": "Malang City Point Jl. Terusan Dieng No.32, ...",
                "price_groups": [
                    {
                        "category": "REGULAR 2D",
                        "low_price": 41000,
                        "high_price": 41000,
                        "price_string": "Rp41.000",
                        "show_time": [
                            {
                                "id": "2056594147065815040",
                                "time": 1779459900000,
                                "display_time": "14:25",
                                "studio": "100101",
                                "expired": 1779458700000,
                                "status": 1,
                                "studio_type": "",
                                "price": 41000
                            }
                        ]
                    }
                ]
            }
        ]
    }
}
```

> 📌 Filter showtime dengan `status: 1` saja. `merchant_name` menentukan slug layout:
> `XXI` → `xxi`, `CGV` → `cgv`, `Cinépolis` → `cinepolis`

---

### `GET /v1/movies/{merchant_slug}/layout?show_time_id={id}&tz=7`

Contoh: `GET /v1/movies/xxi/layout?show_time_id=2057538032403300352&tz=7`

**Response `200 OK`:**
```json
{
    "success": true,
    "data": {
        "user_seat_purchased": 0,
        "user_seat_daily_limit": 10,
        "user_seat_transaction_limit": 8,
        "max_horizontal_seat": 15,
        "max_vertical_seat": 9,
        "seat_rule_config": {
            "type": 1,
            "allowed_adjacent_seat": 0
        },
        "seat_rules": {
            "horizontal_lane": null,
            "vertical_lane": [
                { "start": "A", "end": "J", "before_seat_column": 8 }
            ]
        },
        "price": 40000,
        "seat_map": [
            {
                "seat_code": "A",
                "max_row": 15,
                "seat_rows": [
                    { "seat_row": "A1", "status": 1 },
                    { "seat_row": "A2", "status": 1 }
                ]
            },
            {
                "seat_code": "C",
                "max_row": 15,
                "seat_rows": [
                    { "seat_row": "C1",  "status": 1 },
                    { "seat_row": "C13", "status": 1 },
                    { "seat_row": "C14", "status": 6 },
                    { "seat_row": "C15", "status": 6 }
                ]
            }
        ]
    }
}
```

> 📌 `user_seat_transaction_limit` = batas kursi per transaksi.
> Status `1` = available, status `6` = tidak ada kursi fisik. Hanya status `1` yang bisa dipilih.

---

### `POST /v1/orders`

**Request body:**
```json
{
    "merchant_id":  "2224f7e3-da00-4fb9-9de3-2b888d83ac02",
    "time_show_id": "2057538032403300352",
    "request_id":   "019e4e79-72e7-7bcd-86df-08ac3c473f47",
    "seat_data": [
        { "seat_id": "D6", "seat_name": "D6", "seat_grd_cd": "D6" },
        { "seat_id": "D7", "seat_name": "D7", "seat_grd_cd": "D7" }
    ]
}
```

> 📌 `merchant_id` diambil dari field `merchant.merchant_id` di response schedules.
> `request_id` dibuat fresh UUID v4 setiap request.

**Response `201 Created`:**
```json
{
    "success": true,
    "data": {
        "id": "2057717647474446336",
        "movie_name": "SEMUA AKAN BAIK-BAIK SAJA",
        "poster_path": "https://asset.tix.id/movie_poster_v2/...",
        "theater_name": "ARAYA XXI",
        "studio_name": "Studio 5",
        "event_start": 1779460200,
        "quantity": 2,
        "selected_seats": ["D6", "D7"],
        "total_ticket_price": 80000,
        "convenience_fee": 8000,
        "max_disc_ticket": 0,
        "total": 88000,
        "expired_at": 1779433512,
        "created_at": 1779433092,
        "failed_times": 0,
        "now": 1779433092,
        "merchant": {
            "merchant_id": "2224f7e3-da00-4fb9-9de3-2b888d83ac02",
            "merchant_name": "XXI"
        },
        "tnc_notes": [
            { "code": 1,   "message": "Purchased tickets cannot be changed / cancelled." },
            { "code": 100, "message": "Children (2 years old/above) are required to purchase ticket." }
        ],
        "age_category": "R",
        "rating_score": 9.2,
        "presale_flag": 0
    }
}
```

> 📌 `data.id` = order_id. `data.expired_at` (Unix detik) = batas waktu bayar.

---

### `GET /v1/orders/{order_id}`

Contoh: `GET /v1/orders/2057717647474446336`

**Response `200 OK`:**
```json
{
    "success": true,
    "data": {
        "order_id": "2057717647474446336",
        "status": 1,
        "movie_name": "SEMUA AKAN BAIK-BAIK SAJA",
        "theater_name": "ARAYA XXI",
        "studio_name": "Studio 5",
        "event_start": 1779460200000,
        "display_time": "Friday, 22 May 2026, 14:30",
        "quantity": 2,
        "selected_seats": ["D6", "D7"],
        "total_ticket_price": 80000,
        "convenience_fee": 8000,
        "total_promo": 0,
        "total_discount": 0,
        "dana_voucher": 88000,
        "total": 88000,
        "expired_at": 1779433512350,
        "created_at": 1779458292350,
        "now": 1779433092878,
        "payment_method_name": "DANA",
        "payment_method": "NETWORK_PAY",
        "payment_option": "NETWORK_PAY_PG_DANA",
        "payment_gateway_image_url": "https://asset.tix.id/payment-method/payment_logo_DANA32.webp",
        "checkout_url": "",
        "payment_code": "",
        "payment_gateway_fee": 0,
        "merchant": {
            "merchant_id": "2224f7e3-da00-4fb9-9de3-2b888d83ac02",
            "merchant_name": "XXI"
        },
        "category_name": "REGULAR",
        "seat_grade_name": "REGULER",
        "age_category": "R",
        "rating_score": 9.2,
        "presale_flag": 0,
        "failed_times": 0
    }
}
```

> 📌 `status` order: `1` = pending payment, `2` = paid, `3` = expired/failed.

---

---

## 6. Model Data Penting

### Status Kursi (seat layout)

| Status  | Arti                          |
| ------- | ----------------------------- |
| `1`     | ✅ Tersedia (bisa dipilih)    |
| `6`     | ❌ Tidak ada kursi / diblokir |
| lainnya | ❌ Tidak bisa dibeli          |

### Status Jadwal (showtime)

| Status | Arti                                     |
| ------ | ---------------------------------------- |
| `0`    | ❌ Expired / sudah lewat waktu pembelian |
| `1`    | ✅ Bisa dibeli                           |

---

## 7. Algoritma Pemilihan Bioskop & Kursi

### 7a. Pemilihan Bioskop (Theater Priority)

Bot mencoba bioskop berdasarkan urutan `theater_priority` di config:

1. Iterasi `theater_priority` dari index 0 (paling prioritas).
2. Untuk setiap bioskop prioritas:
   a. Cocokkan nama (substring, case-insensitive) ke daftar theater dari API.
   b. Filter showtime yang `status: 1` dan dalam rentang `preferred_time_start`–`preferred_time_end`.
   c. Jika ada showtime valid → gunakan bioskop + showtime ini.
   d. Jika tidak ada showtime valid → lanjut ke bioskop berikutnya dalam priority list.
3. Jika semua bioskop di priority list tidak memenuhi syarat → fallback ke bioskop pertama yang tersedia (apapun).
4. Jika `theater_priority` kosong `[]` → pilih bioskop pertama yang punya showtime valid tanpa preferensi.

### Contoh

```
theater_priority = ["TRANSMART MX MALL XXI", "MALANG TOWN SQUARE CINEPOLIS", "ARAYA XXI"]
preferred_time_start = "12:00"
preferred_time_end   = "22:00"

Bioskop dari API:
  1. ARAYA XXI              → showtime 12:45 (expired), 14:30 ✅, 18:45 ✅
  2. MALANG CITY POINT CGV  → showtime 14:25 ✅, 16:45 ✅
  3. MALANG TOWN SQUARE CINEPOLIS → showtime 16:35 ✅, 21:10 ✅
  4. TRANSMART MX MALL XXI  → showtime 14:35 ✅, 18:50 ✅, 21:00 ✅

Proses:
  Coba priority[0] "TRANSMART MX MALL XXI" → match theater #4 → punya showtime valid ✅
  → Pilih TRANSMART MX MALL XXI, showtime 14:35
```

---

### 7b. Algoritma Pilih Kursi

1. **Manual seats** (dari config): dicoba duluan. Jika tidak semua tersedia → fallback auto.
2. **Auto-select**:
   a. Skip `avoid_first_rows` baris pertama (default: A, B, C).
   b. Filter ke range `preferred_rows` (default: D–H).
   c. Urutkan baris dari **paling tengah ke tepi** dalam range tersebut.
    - Contoh: range D–H → urutan: F, E, G, D, H
      d. Untuk setiap baris (prioritas tengah duluan):
    - Cari semua kelompok `quantity` kursi **berurutan** yang semua status = 1.
    - Pilih kelompok yang posisinya **paling dekat ke tengah baris**.
      e. Jika preferred range tidak punya cukup kursi → lanjut ke baris usable lainnya.

### Contoh (Bioskop ARAYA XXI, quantity=2, avoid_first_rows=3, preferred_rows=[D,H])

```
Baris tersedia: D, E, F, G, H, J
Urutan coba: F → E → G → D → H → J

Baris F (15 kursi, F14 & F15 blocked):
  Available: F1–F13 (status 1)
  Tengah baris: posisi 7 (index)
  Best group (2 berurutan paling tengah): F7, F8 ✅
```

---

## 8. Struktur File

```
tixid-bot/
├── Cargo.toml
├── config.toml          ← konfigurasi pengguna
└── src/
    ├── main.rs           ← entry point
    ├── config.rs         ← load & parse config.toml
    ├── models.rs         ← semua struct request/response
    ├── client.rs         ← HTTP client dengan default headers
    ├── api.rs            ← semua fungsi pemanggil API
    ├── theater_selector.rs ← algoritma pilih bioskop berdasarkan theater_priority
    ├── seat_selector.rs  ← algoritma pilih kursi
    └── bot.rs            ← orkestrasi alur utama
```

---

## 9. Output Terminal

```
🎬 TIX.ID Bot
========================================
🔑 Logging in as +6289696508086...
✅ Welcome, Tri Adi!

🎥 Fetching movie ID: 2039608798242488320...
✅ SEMUA AKAN BAIK-BAIK SAJA (113 min, NOW_PLAYING)

📅 Getting available dates...
✅ Target date: 2026-05-22

🏟️  Getting schedules for 2026-05-22...
✅ Found 4 theater(s)

🎬 Selected showtime:
   Theater:  ARAYA XXI
   Time:     14:30
   Studio:   5
   Category: 2D
   Price:    Rp40000

💺 Fetching seat layout...
✅ Available: 117/135 seats (tx limit: 8)

💺 Selected seats: F7, F8

🛒 Placing order...

========================================
🎉 ORDER SUCCESSFUL!
========================================
   Order ID:  2057717647474446336
   Movie:     SEMUA AKAN BAIK-BAIK SAJA
   Theater:   ARAYA XXI | Studio 5
   Seats:     F7, F8
   Ticket:    2 x Rp40000 = Rp80000
   Fee:       Rp8000
   TOTAL:     Rp88000
   Expires:   15:44:12 WIB
========================================
⚡ Proceed to payment before expiry!
```

---

## 10. Fase Pengembangan

| Fase                        | Fitur                                                              |
| --------------------------- | ------------------------------------------------------------------ |
| ✅ **v1.0** — Core Bot      | Login, browse movie, pilih jadwal, auto-select kursi, create order |
| 🔲 **v1.1** — Payment       | Tampilkan QRIS PNG di terminal untuk bayar via DANA / GoPay        |
| 🔲 **v1.2** — Auto Payment  | Integrasi payment gateway (DANA deep link, dll)                    |
| 🔲 **v1.3** — Sniper Mode   | Loop polling kursi tersedia untuk tayangan yang belum buka booking |
| 🔲 **v1.4** — Multi-account | Support beberapa akun sekaligus                                    |

---

## 11. Catatan Keamanan

- Jangan commit `config.toml` ke git (berisi password terenkripsi & token).
- Tambahkan `config.toml` ke `.gitignore`.
- Password yang disalin dari browser adalah **RSA-encrypted** — tidak bisa dibaca manusia biasa, tapi tetap sensitif.
