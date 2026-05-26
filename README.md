# 🎬 TIX.ID Bot

Bot otomatis untuk membeli tiket bioskop di [tix.id](https://tix.id), ditulis dalam **Rust**.  
Target: secepat mungkin dari start → seat hold → QRIS payment.
**WEB version** : 1.0.11

---

## ✅ TODO List

### v1.0 — Core Bot ✅ DONE

- [x] `src/config.rs` — Load & parse `config.toml` (termasuk `theater_priority` array, `latitude`/`longitude`)
- [x] `src/models.rs` — Semua struct request/response (auth, login, movie, schedule, seat, order, checkout)
- [x] `src/client.rs` — HTTP client dengan default headers (Authorization, device_id, platform, dll)
- [x] `src/api.rs`
    - [x] `get_guest_token()` → anonymous token (wajib sebelum login)
    - [x] `login(msisdn, password)` → JWT token
    - [x] `get_movie(movie_id)` → detail film + schedule_id
    - [x] `get_schedule_dates(schedule_id, city_id)` → daftar tanggal tersedia
    - [x] `get_showtimes(schedule_id, city_id, date)` → daftar bioskop + jadwal
    - [x] `get_seat_layout(merchant_slug, show_time_id)` → peta kursi
    - [x] `create_order(merchant_id, time_show_id, seats)` → order created
    - [x] `get_payment_channels(order_id)` → daftar metode pembayaran
    - [x] `checkout(order_id, lat, lng, method, option)` → QRIS / payment code
- [x] `src/theater_selector.rs` — Algoritma pilih bioskop berdasarkan `theater_priority`
- [x] `src/seat_selector.rs` — Algoritma pilih kursi (manual → auto center-out → fallback)
- [x] `src/bot.rs` — Orkestrasi alur utama full end-to-end
- [x] `src/main.rs` — Entry point

### v1.1 — Payment ✅ DONE

- [x] Checkout API dengan `request_id`, `latitude`, `longitude`
- [x] Default payment method QRIS (`NETWORK_PAY_PG_QRIS`)
- [x] Render QR code di terminal (Unicode block characters)
- [x] Tampilkan QR image URL (`api.qrserver.com`)

### v1.2 — Sniper Mode ✅ DONE

- [x] Loop polling jadwal yang belum buka booking (film `UPCOMING`)
- [x] Langsung eksekusi begitu jadwal/showtime muncul
- [x] Auto-refresh token saat standby lama
- [x] `start_at` timer — bot tidur sampai waktu tertentu (WIB), lalu mulai war

### v1.3 — Logging ✅ DONE

- [x] Non-blocking async logging ke file `tixid-bot.log` (channel-based, tidak block hot path)
- [x] Log semua request body dan response JSON ke file
- [x] Audit events: login, showtime selected, seats selected, order created, checkout completed
- [x] Error API (response `success: false`) di-handle dengan pesan yang jelas

### v1.4 — Smart Seating ✅ DONE

- [x] Ranking semua bioskop sesuai `theater_priority`
- [x] Parallel seat layout fetch — semua request dikirim serentak
- [x] Signaling: pemenang dikonfirmasi sesuai ranking, bukan siapa yang reply duluan
- [x] Abort otomatis semua in-flight request begitu pemenang dikonfirmasi
- [x] Fallback ke bioskop berikutnya jika tidak ada N kursi berurutan

### v1.5 — Multi-format Seat Layout ✅ DONE

- [x] Auto-deteksi format seat layout: XXI (nested `seat_map`) vs Cinepolis/CGV (flat list)
- [x] `SelectedSeat` struct membawa `seat_id` (booking ID), `display` (label), `grd_cd` (price tier)
- [x] Kirim `seat_grd_cd` yang benar ke `create_order` (kode tier harga, bukan seat_id)
- [x] Grid/aisle awareness: kursi di sisi kanan/kiri lorong tengah Cinepolis tidak dianggap berurutan
- [x] Spacer/aisle marker (`seat_yn: "0"`) difilter otomatis dari Cinepolis/CGV layout

### Prerequisites

- [Rust](https://rustup.rs/) (install via `rustup`)
- Windows: [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) (MSVC toolchain)

### Build & Run

```bash
# Masuk direktori
cd tixid-bot

# Edit konfigurasi
cp config.toml.example config.toml
# isi msisdn, password (RSA-encrypted dari browser), movie_id, dll

# Build release (optimized)
cargo build --release

# Jalankan
.\target\release\tixid-bot.exe         # Windows
./target/release/tixid-bot             # Linux/macOS
```

---

## 🔧 Konfigurasi (`config.toml`)

```toml
[auth]
msisdn   = "+628xxxxxxxxxx"
password = "<RSA-encrypted-password>"   # salin dari browser DevTools > Network > login payload

[target]
movie_id = "2039608798242488320"        # ID film (dari URL tix.id)
city_id  = "973818515335155712"         # Kota Malang
date     = ""                           # kosong = otomatis (tanggal pertama tersedia)

[theater]
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
manual_seats       = []                 # contoh: ["D6","D7"] — dicoba duluan
avoid_first_rows   = 3                  # skip N baris paling depan/dekat layar (A=belakang, huruf besar=dekat layar)
preferred_rows     = ["D", "H"]         # range baris disukai (inklusif, boleh terbalik)

[device]
device_id = "019e4e4e-f638-7fca-b747-69e48a9eef32"
latitude  = "-7.8078"                   # koordinat untuk checkout API
longitude = "112.04"

[payment]
payment_method = "NETWORK_PAY"
payment_option = "NETWORK_PAY_PG_QRIS" # opsi: QRIS | NETWORK_PAY_PG_DANA | NETWORK_PAY_PG_OVO

[polling]
enabled                   = true        # standby & retry jika jadwal belum ada
interval_secs             = 2           # jeda antar retry
refresh_token_before_secs = 1500        # re-login tiap 25 menit saat standby
start_at                  = ""          # "YYYY-MM-DD HH:MM:SS" WIB — kosong = mulai sekarang
```

### Cara dapat `password` (RSA-encrypted)

1. Buka [app.tix.id](https://app.tix.id) di browser
2. Buka DevTools → tab **Network**
3. Login manual
4. Cari request `POST /v1/users/login` → lihat **Request Payload**
5. Salin nilai field `password` → paste ke `config.toml`

### Cara dapat `movie_id`

Buka halaman film di tix.id, ID ada di URL:

```
https://app.tix.id/movie/detail/2039608798242488320
                                 ^^^^^^^^^^^^^^^^^^^ ini movie_id
```

---

## 🚀 Contoh Output

```
🎬 TIX.ID Bot
========================================
✅ Guest token OK (expires in 30min)
✅ Welcome, Tri Adi!
✅ SEMUA AKAN BAIK-BAIK SAJA (113 min, NOW_PLAYING)
✅ Found 4 theater(s)

🎬 Selected showtime:
   Movie:     SEMUA AKAN BAIK-BAIK SAJA
   Theater:   TRANSMART MX MALL XXI
   Time:      18:50
   Studio:    3
   Date:      2026-05-22
   Category:  2D
   Price:     Rp40.000

✅ Available: 119/135 seats (tx limit: 8)
💺 Selected seats: F7, F8

🛒 Placing order...

========================================
🎉 ORDER SUCCESSFUL!
========================================
   Order ID:  2057771159164047360
   Movie:     SEMUA AKAN BAIK-BAIK SAJA
   Theater:   TRANSMART MX MALL XXI | Studio 3
   Seats:     F7, F8
   Ticket:    2 x Rp40.000 = Rp80.000
   Fee:       Rp8.000
   TOTAL:     Rp88.000
   Expires:   17:37:50 WIB
========================================

💳 Checking out with NETWORK_PAY_PG_QRIS...
✅ Checkout OK

========================================
💳 PAYMENT INFO
========================================
   Method:    NETWORK_PAY_PG_QRIS
   Amount:    Rp88.000
   Total time: 1.851s

   QRIS Code:
   00020101021226570011ID.DANA...

   QR URL:    https://api.qrserver.com/v1/create-qr-code/?size=300x300&data=...

   QRIS (Terminal):
   ██████████████████████
   ██ ▄▄▄▄▄ █▀ █▀ ▄▄▄▄▄ ██
   ...

   ⚠️  Scan QRIS above before 17:37:50 WIB
========================================
```

---

## 🧠 Algoritma Pilih Bioskop & Kursi

### Ranking Bioskop + Cek Kursi (Parallel Race + Early Abort)

Bot **meranking semua bioskop** lalu mengirim **semua seat layout request secara SERENTAK** (parallel). Hasil diproses sesuai urutan prioritas — begitu pemenang tertinggi dikonfirmasi, semua request lain langsung **dibatalkan (abort)**.

```
1. Rank semua bioskop berdasarkan theater_priority (substring match, case-insensitive):
   a. Bioskop yang cocok priority[0] → rank 1 (tertinggi)
   b. Bioskop yang cocok priority[1] → rank 2
   c. dst...
   d. Bioskop yang tidak masuk priority → rank terakhir (fallback)

2. Kirim SEMUA seat layout request serentak (tokio parallel tasks)

3. Setiap task mengirim sinyal saat selesai: (rank, hasil)
   → Hasil: Some(layout+seats) jika ada N kursi berurutan, None jika tidak

4. Konfirmasi pemenang:
   - Saat rank R punya kursi DAN semua rank < R sudah menjawab tanpa kursi → rank R menang
   - Abort semua task yang masih berjalan (rank > R)
   - Langsung checkout dengan rank R

5. Jika semua bioskop tidak ada yang punya N kursi berurutan → error
```

**Mengapa lebih cepat:**

```
Worst-case sequential (lama): latency × N = 150ms × 4 = 600ms
Parallel race (baru):         max(latency)  = 150ms       ✅ 4x lebih cepat
Happy path (rank#1 punya kursi): sama saja — ~150ms, tapi request lain sudah dibatalkan
```

**Contoh (quantity=2, 4 bioskop):**

```
Priority: [TRANSMART, CINEPOLIS, ARAYA, CGV]

t=0ms   → Kirim 4 seat layout requests serentak
t=120ms → CGV reply dulu (rank#4): ada kursi — tapi rank 1,2,3 belum reply → tunggu
t=135ms → ARAYA reply (rank#3): ada kursi — rank 1,2 belum reply → tunggu
t=140ms → CINEPOLIS reply (rank#2): tidak ada kursi berurutan
t=145ms → TRANSMART reply (rank#1): tidak ada kursi berurutan
          Semua rank < rank#3 sudah menjawab tanpa kursi
          → ARAYA menang (rank#3) 🏆  Abort request CGV di-cancel (sudah reply sih)
          → Checkout ARAYA
```

### Pilih Kursi (dalam satu bioskop)

> ⚠️ **Orientasi baris tix.id:** Baris **A = paling BELAKANG** (terjauh dari layar). Huruf makin besar = makin dekat layar.

> 📌 **Format kursi:** XXI menggunakan format nested (`seat_map` per baris), sedangkan Cinepolis/CGV menggunakan flat list dengan `seat_id` booking ID terpisah (e.g. `"2-0-3-0"`) dan `seat_grd_cd` kode tier (e.g. `"0000000013"`). Bot mendeteksi format otomatis.

```
1. Coba manual_seats dari config (jika semua tersedia → pakai)
2. Auto-select:
   a. Skip avoid_first_rows baris paling DEPAN/dekat layar (huruf terbesar, default skip 3)
   b. Filter ke preferred_rows range (default D–H = tengah studio, boleh dibalik)
   c. Urutkan baris dari tengah range ke tepi: F→E→G→D→H
   d. Tiap baris: cari grup N kursi BERURUTAN paling dekat tengah baris
      — "Berurutan" = kolom numerik berdekatan DAN sisi aisle yang sama
        (kursi di kiri/kanan lorong tengah Cinepolis tidak dianggap berurutan)
   e. Fallback: jika preferred range habis → lanjut ke baris usable lainnya
```

---

## 🏗️ Struktur Project

```
tixid-bot/
├── Cargo.toml
├── config.toml          ← konfigurasi pengguna (jangan di-commit!)
├── config.example.toml  ← template konfigurasi
├── tixid-bot.log        ← audit log (generated)
├── README.md
├── PRD.md               ← spesifikasi teknis lengkap
└── src/
    ├── main.rs           ← entry point + inisialisasi logging
    ├── config.rs         ← load & parse config.toml
    ├── models.rs         ← semua struct serde
    ├── client.rs         ← HTTP client + default headers
    ├── api.rs            ← fungsi pemanggil API (log request+response otomatis)
    ├── logger.rs         ← non-blocking async file logger (tracing-appender)
    ├── theater_selector.rs ← algoritma pilih bioskop
    ├── seat_selector.rs  ← algoritma pilih kursi
    └── bot.rs            ← orkestrasi alur utama
```

---

## 🔒 Keamanan

- Tambahkan `config.toml` ke `.gitignore` — berisi password dan token sensitif
- Password di `config.toml` sudah RSA-encrypted oleh browser, tapi tetap jangan di-share

---

## 📋 Roadmap

| Versi | Status     | Fitur                                                               |
| ----- | ---------- | ------------------------------------------------------------------- |
| v1.0  | ✅ Done    | Login → cari film → pilih jadwal → auto-select kursi → create order |
| v1.1  | ✅ Done    | QRIS checkout + render QR di terminal                               |
| v1.2  | ✅ Done    | Sniper mode (polling + `start_at` war timer)                        |
| v1.3  | ✅ Done    | Non-blocking async file logging                                     |
| v1.4  | ✅ Done    | Smart Seating: parallel race, winner by rank, abort losers          |
| v1.5  | ✅ Done    | Multi-format layout (XXI/Cinepolis/CGV) + grid/aisle-aware seating  |
| v1.6  | 📋 Planned | Multi-account support (concurrent)                                  |
