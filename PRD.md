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
avoid_first_rows   = 3                  # skip N baris paling depan/dekat layar (A=belakang, huruf besar=dekat layar)
preferred_rows     = ["D", "H"]         # range baris disukai — tengah studio (inklusif)

[device]
device_id = "019e4e4e-f638-7fca-b747-69e48a9eef32"
```

---

## 4. Alur Bot

```
1. Load config.toml
2. (Opsional) Standby sampai polling.start_at (war timer)
3. POST /v1/auth → guest token (anonymous, client_id="tixid_guest")
4. LOGIN (pakai guest token) → user JWT + refresh_token
5. GET movie by movie_id → dapat schedule_id (data.id); poll jika UPCOMING
6. GET schedule dates → pilih date (config / otomatis = tanggal pertama tersedia)
7. GET theaters + showtimes (page=1) → ranking semua bioskop sesuai theater_priority
8. Untuk setiap bioskop dalam ranking:
   a. GET seat layout
   b. Cari N kursi BERURUTAN (berdampingan) → jika ada, lanjut ke langkah 9
   c. Jika tidak ada → coba bioskop berikutnya
9. POST create order → tampilkan ringkasan + deadline bayar
10. POST checkout (QRIS) → tampilkan QRIS code + render QR di terminal
11. Token refresh via /v1/users/refresh saat standby lama (fallback: full re-login)
```

---

## 5. Endpoint API

**Base URL semua endpoint:** `https://api-b2b.tix.id`

| Method | Endpoint                                                                   | Keterangan                                                                     |
| ------ | -------------------------------------------------------------------------- | ------------------------------------------------------------------------------ |
| POST   | `/v1/auth`                                                                 | Guest token (tanpa auth), body: `{"client_id":"tixid_guest","auth_code":null}` |
| POST   | `/v1/users/login`                                                          | Login, dapat JWT — pakai guest token sebagai Authorization                     |
| GET    | `/v1/users`                                                                | Data profil user                                                               |
| GET    | `/v1/movies/{movie_id}`                                                    | Detail film + dapat `data.id` (schedule_id)                                    |
| GET    | `/v1/schedules/date?schedule_id={id}&city_id={id}`                         | Daftar tanggal tersedia                                                        |
| GET    | `/v1/schedules/movies/{schedule_id}?city_id={id}&date={YYYY-MM-DD}&page=1` | Bioskop + jadwal per tanggal                                                   |
| GET    | `/v1/movies/{merchant_slug}/layout?show_time_id={id}&tz=7`                 | Peta kursi (XXI → `xxi`, CGV → `cgv`, Cinépolis → `cinepolis`)                 |
| POST   | `/v1/orders`                                                               | Buat pesanan (seat hold)                                                       |
| GET    | `/v1/orders/{order_id}`                                                    | Detail pesanan                                                                 |

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
                    { "seat_row": "C1", "status": 1 },
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
> Status `1` = available, status `6` = tidak ada kursi fisik (XXI). Hanya status `1` yang bisa dipilih.

---

### `GET /v1/movies/{merchant_slug}/layout?show_time_id={id}&tz=7` (Cinepolis/CGV — flat format)

Contoh: `GET /v1/movies/cinepolis/layout?show_time_id=2057783188058624000&tz=7`

**Response `200 OK`:**

```json
{
    "success": true,
    "data": {
        "user_seat_transaction_limit": 6,
        "price_group": [
            { "seat_grd_cd": "0000000000", "seat_grd_nm": "REGULAR",   "seat_grd_price": 52000 },
            { "seat_grd_cd": "0000000013", "seat_grd_nm": "PREFERRED",  "seat_grd_price": 54000 }
        ],
        "seat_map": [
            { "seat_id": "2-0-0-0", "row_name": "A", "seat_no": "1", "seat_yn": "1", "seat_grd_cd": "0000000013", "seat_status": 1 },
            { "seat_id": "2-0-1-0", "row_name": "A", "seat_no": "2", "seat_yn": "1", "seat_grd_cd": "0000000013", "seat_status": 0 },
            { "seat_id": "4-4",     "row_name": "A", "seat_no": null, "seat_yn": "0", "seat_grd_cd": null,         "seat_status": 0 },
            { "seat_id": "2-0-5-0", "row_name": "A", "seat_no": "5", "seat_yn": "1", "seat_grd_cd": "0000000013", "seat_status": 1 }
        ]
    }
}
```

> 📌 Format flat: satu entry per kursi. `seat_id` = booking ID yang dikirim ke `create_order`.  
> `seat_yn: "0"` atau `seat_no: null` = spacer/lorong tengah — diabaikan.  
> `seat_status: 1` = available, `seat_status: 0` = terjual/terpesan.  
> `seat_grd_cd` per kursi = kode tier harga yang dikirim ke `create_order`.  
> Kolom `seat_no` bernomor 1–8 di tiap sisi lorong; kolom 4 dan 5 berada di sisi berbeda (ada spacer di antara mereka).

---

### `POST /v1/orders`

**Request body (XXI):**

```json
{
    "merchant_id": "2224f7e3-da00-4fb9-9de3-2b888d83ac02",
    "time_show_id": "2057538032403300352",
    "request_id": "019e4e79-72e7-7bcd-86df-08ac3c473f47",
    "seat_data": [
        { "seat_id": "F7", "seat_name": "F7", "seat_grd_cd": "" },
        { "seat_id": "F8", "seat_name": "F8", "seat_grd_cd": "" }
    ]
}
```

**Request body (Cinepolis/CGV):**

```json
{
    "merchant_id": "37cee700-7e19-4353-b806-dbb1dcdcfbd2",
    "time_show_id": "2057783188058624000",
    "request_id": "d8e54574-0639-4d9b-8aa1-aa327e9678dd",
    "seat_data": [
        { "seat_id": "2-0-3-0", "seat_name": "A4", "seat_grd_cd": "0000000013" }
    ]
}
```

> 📌 `seat_id` = booking ID dari API layout (XXI: same as label, Cinepolis/CGV: format `"2-0-3-0"`).  
> `seat_name` = label tampilan (e.g. `"A4"`).  
> `seat_grd_cd` = kode tier harga dari `price_group` (e.g. `"0000000013"` = PREFERRED, `"0000000000"` = REGULAR). Untuk XXI gunakan string kosong `""`.  
> `merchant_id` diambil dari `merchant.merchant_id` di response schedules.  
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
            {
                "code": 1,
                "message": "Purchased tickets cannot be changed / cancelled."
            },
            {
                "code": 100,
                "message": "Children (2 years old/above) are required to purchase ticket."
            }
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

| Status      | Format        | Arti                                       |
| ----------- | ------------- | ------------------------------------------ |
| `1`         | XXI & flat    | ✅ Tersedia (bisa dipilih)                 |
| `0`         | Cinepolis/CGV | ❌ Terjual / terpesan                      |
| `6`         | XXI           | ❌ Tidak ada kursi fisik / diblokir        |
| lainnya     | semua         | ❌ Tidak bisa dibeli                       |

> 📌 Entry dengan `seat_yn: "0"` atau `seat_no: null` pada format flat = spacer/lorong tengah — difilter sebelum pemilihan kursi.

### Status Jadwal (showtime)

| Status | Arti                                     |
| ------ | ---------------------------------------- |
| `0`    | ❌ Expired / sudah lewat waktu pembelian |
| `1`    | ✅ Bisa dibeli                           |

---

## 7. Algoritma Pemilihan Bioskop & Kursi

### 7a. Ranking Bioskop + Cek Kursi (Parallel Race + Early Abort)

Bot **meranking semua bioskop** lalu mengirim **semua seat layout request secara SERENTAK**. Hasil didengarkan melalui channel; pemenang dikonfirmasi berdasarkan ranking (bukan siapa yang reply tercepat), lalu semua request yang masih berjalan langsung **di-abort**.

**Urutan ranking:**

1. Bioskop yang cocok `priority[0]` (substring, case-insensitive) → rank 1
2. Bioskop yang cocok `priority[1]` → rank 2
3. dst... sesuai panjang `theater_priority`
4. Bioskop yang tidak masuk priority list → rank terakhir (fallback, urutan asli dari API)

**Alur eksekusi:**

1. Filter showtime dengan `status: 1` dalam rentang `preferred_time_start`–`preferred_time_end` untuk semua bioskop
2. Spawn semua seat layout request sebagai parallel async tasks (`tokio::spawn`)
3. Setiap task mengirim `(rank, Option<(layout, seats)>)` ke channel `mpsc::unbounded`
4. Saat task rank R menjawab dengan kursi, dan semua rank < R sudah menjawab tanpa kursi → **rank R dikonfirmasi sebagai pemenang**
5. Abort semua `JoinHandle` yang masih in-flight (`handle.abort()`)
6. Lanjut checkout dengan theater rank R

**Kompleksitas waktu:**

```
Sequential (lama):  latency × N          (600ms untuk 4 bioskop @ 150ms)
Parallel race:      max(latency of needed responses)   ≈ 150ms  ✅
```

### Contoh

```
theater_priority = ["TRANSMART MX MALL XXI", "CINEPOLIS", "ARAYA XXI", "CGV"]
quantity = 2

t=0ms   → 4 seat layout requests dikirim serentak
t=120ms → CGV (rank#4) reply: ada kursi — tunggu rank 1,2,3
t=135ms → ARAYA (rank#3) reply: ada kursi — tunggu rank 1,2
t=140ms → CINEPOLIS (rank#2) reply: tidak ada kursi berurutan
t=145ms → TRANSMART (rank#1) reply: tidak ada kursi berurutan
          Semua rank < 3 sudah menjawab tanpa kursi
          → Pemenang: ARAYA XXI (rank#3) 🏆
          → Semua JoinHandle lain di-abort
          → Checkout: ARAYA XXI, F7, F8
Total: 145ms (bukan 540ms sequential)
```

### 7b. Algoritma Pilih Kursi (dalam satu bioskop)

> ⚠️ **Orientasi baris di tix.id:** Baris **A = paling BELAKANG** (terjauh dari layar).
> Huruf makin besar = makin dekat layar. Baris dengan huruf terbesar di layout = paling DEPAN (dekat layar).

> 📌 **Multi-format:** XXI menggunakan format nested (`seat_map` berisi array per baris), Cinepolis/CGV menggunakan flat list di mana setiap entry adalah satu kursi. Bot mendeteksi format secara otomatis via `#[serde(untagged)]` enum dan menormalisasi ke struktur yang sama.

> 📌 **Grid/Aisle awareness (Cinepolis/CGV):** Studio memiliki lorong tengah yang membagi kursi menjadi dua sisi (kiri/kanan). Kolom di sisi berbeda dari lorong tidak dianggap berurutan meskipun nomor kolomnya berdekatan. Deteksi dilakukan via field `seat_grd_cd` yang berfungsi ganda sebagai kode tier (`"0000000000"` / `"0000000013"`) dan identitas sisi aisle.

1. **Manual seats** (dari config): dicoba duluan. Jika tidak semua tersedia → fallback auto.
2. **Auto-select**:
   a. Skip `avoid_first_rows` baris **paling depan** (huruf terbesar, terdekat layar).
   b. Filter ke range `preferred_rows` (default: D–H = tengah studio).
   c. Urutkan baris dari **paling tengah ke tepi** dalam range tersebut.
    - Contoh: range D–H → urutan: F, E, G, D, H
      d. Untuk setiap baris (prioritas tengah duluan):
    - Cari semua kelompok `quantity` kursi **berurutan** (berdampingan, sisi aisle sama) yang semua status = 1.
    - Pilih kelompok yang posisinya **paling dekat ke tengah baris**.
      e. Jika preferred range tidak punya cukup kursi berurutan → lanjut ke baris usable lainnya.

### Contoh (ARAYA XXI, quantity=2, avoid_first_rows=3, preferred_rows=[D,H])

```
⚠️  Layout: A,B,C = belakang ← D,E,F,G,H = tengah → I,J = depan/dekat layar

Contoh jika 3 baris terdepan di layout adalah X, Y, Z maka baris itu yang dilewati oleh `avoid_first_rows = 3`.
Urutan coba (tengah range D–H duluan): F → E → G → D → H → lalu baris usable lain

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

| Fase                        | Fitur                                                                              |
| --------------------------- | ---------------------------------------------------------------------------------- |
| ✅ **v1.0** — Core Bot      | Login, browse movie, pilih jadwal, auto-select kursi berurutan, create order       |
| ✅ **v1.1** — Payment       | QRIS checkout, render QR di terminal, QR image URL                                 |
| ✅ **v1.2** — Sniper Mode   | Loop polling + `start_at` war timer, auto token refresh via refresh_token endpoint |
| ✅ **v1.3** — Logging       | Non-blocking async file logging, log semua request/response JSON                   |
| ✅ **v1.4** — Smart Seating | Parallel race: semua seat layout fetch serentak, winner by rank, abort losers      |
| 🔲 **v1.5** — Multi-account | Support beberapa akun sekaligus (concurrent)                                       |

---

## 11. Catatan Keamanan

- Jangan commit `config.toml` ke git (berisi password terenkripsi & token).
- Tambahkan `config.toml` ke `.gitignore`.
- Password yang disalin dari browser adalah **RSA-encrypted** — tidak bisa dibaca manusia biasa, tapi tetap sensitif.
