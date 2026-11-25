# 🛡️ Discord Protection Bot (Rust / Twilight-rs)

بوت حماية متكامل وفائق السرعة لسيرفرات ديسكورد (Anti-Nuke & Server Protection)، مبرمج بلغة **Rust** باستخدام مكتبات **`twilight-rs`** ومحرك التزامن **`tokio`**.  
جميع الإعدادات وتذييل الرسائل (`embedFooter`) قابلة للتعديل أو الإزالة بحرية كاملة من ملف `config.json`.

---

## ⚡ المميزات الرئيسية

- **أداء عالٍ واستجابة لحظية:** معالجة غير محظورة (Non-blocking) لأحداث ديسكورد باستخدام `tokio` و `DashMap`.
- **سجلات فورية عبر Webhook:** إرسال جميع تنبيهات الحماية عبر Discord Webhook مع إمكانية تغييره واختباره بأمر `/setwebhook`.
- **لوحة تحكم تفاعلية (`/setup`):** تفعيل وتعطيل وتعديل جميع أنظمة الحماية بالأزرار والقوائم المنسدلة مباشرة داخل ديسكورد.
- **مكافحة السبام والروابط:** حذف تلقائي للرسائل وفرض عقوبات قابلة للتخصيص (`timeout` / `kick` / `ban` / `none`).
- **نظام فخ الاختراق (Hack Mode):** عزل الحسابات المخترقة التي ترسل في قناة الفخ وسحب رتبها مؤقتاً، مع استرجاعها الكامل بأمر `/unhack`.
- **مراقبة سجلات التدقيق (Audit Log Monitoring):**
  - مكافحة الحظر والطرد الجماعي (`banMonitoring` / `kickMonitoring`).
  - منع إضافة البوتات غير المصرح بها (`botMonitoring`).
  - حماية القنوات والرتب من الإنشاء والحذف والتعديل الجماعي (`channelMonitoring` / `roleMonitoring` / `roleEditMonitoring`).
  - مراقبة تعديل إعدادات السيرفر (`serverEditMonitoring`).
- **قائمة بيضاء للأدوار (`/whitelist`):** استثناء رتب الإدارة العليا من إجراءات الحماية التلقائية.
- **تخصيص كامل من `config.json`:** يمكنك تغيير نص الفوتر (`embedFooter`) إلى اسم سيرفرك أو تركه فارغاً `""` لإخفائه تماماً بدون تعديل الكود.

---

## 📁 هيكلية المشروع

```text
├── Cargo.toml            # تعريف المشروع والحزم
├── Dockerfile            # بناء وتشغيل البوت عبر Docker
├── docker-compose.yml    # تشغيل تلقائي 24/7 عبر Docker Compose
├── .env.example          # نموذج ملف متغيرات البيئة (لتخزين التوكن بأمان)
├── config.json           # ملف الإعدادات الرئيسي (قابل للتعديل بالكامل)
├── hacked_users.json     # قاعدة بيانات رتب الأعضاء المعزولين
├── start.bat             # سكربت تشغيل تلقائي لويندوز
└── src/
    ├── main.rs           # نقطة الانطلاق والاتصال بالـ Gateway
    ├── config.rs         # قراءة وحفظ الإعدادات وملف .env
    ├── state.rs          # إدارة الحالة المشتركة والكاش
    ├── error.rs          # تعريف ومعالجة الأخطاء
    ├── utils.rs          # الصلاحيات، العقوبات، والويب هوك
    ├── commands/         # أوامر السلاش (/setup, /setwebhook, /unhack, /whitelist)
    └── events/           # معالجة الرسائل، سجلات التدقيق، والتفاعلات
```

---

## ⚙️ متطلبات التشغيل

1. تثبيت [Rust & Cargo](https://rustup.rs/) (الإصدار `1.80` أو أحدث).
2. إنشاء تطبيق وبوت من [Discord Developer Portal](https://discord.com/developers/applications).
3. تفعيل الـ **Privileged Gateway Intents** التالية من صفحة البوت:
   - ✅ **Server Members Intent**
   - ✅ **Message Content Intent**
4. منح البوت صلاحيات **Administrator** ورفع رتبة البوت فوق رتب الأعضاء والمشرفين المراد مراقبتهم.

---

## 🛠️ خطوات الإعداد والتشغيل

### 1. إعداد التوكن (موصى به عبر `.env`)

انسخ ملف `.env.example` إلى `.env` وضع توكن البوت داخله:

```bash
cp .env.example .env
```

```env
DISCORD_TOKEN=YOUR_BOT_TOKEN_HERE
RUST_LOG=info
```

> ⚠️ **تنبيه أمني مهم:** يُنصح بوضع التوكن داخل ملف `.env` فقط وعدم وضعه في `config.json` إذا كنت ترفع المشروع على GitHub (ملف `.env` مضاف تلقائياً إلى `.gitignore`).

### 2. تعديل ملف `config.json`

افتح [config.json](config.json) وعدّل القيم بما يناسب سيرفرك:

| المفتاح | الوصف |
| :--- | :--- |
| `DISCORD_TOKEN` | توكن البوت (اتركه فارغاً `""` إذا وضعته في `.env`) |
| `allowedGuildId` | آيدي السيرفر المسموح للبوت بالعمل فيه (اتركه فارغاً `""` للعمل في أي سيرفر) |
| `logWebhookUrl` | رابط Discord Webhook لإرسال سجلات الحماية (يمكن ضبطه أيضاً عبر `/setwebhook`) |
| `embedFooter` | النص الظاهر أسفل رسائل الإيمبد (الافتراضي: `"zrx"` — يمكنك تغييره لأي نص أو تركه `""` لإزالته) |
| `whitelistRoles` | مصفوفة آيديات الرتب المستثناة من الحماية (يمكن إدارتها أيضاً عبر `/whitelist`) |
| `hackMode` | إعدادات نظام قناة الفخ (`active`, `hackChannelId`, `hackedRoleId`, `hackedViewChannelId`) |
| `antispam` / `antilinks` / `*Monitoring` | إعدادات التفعيل والحدود الزمنية ونوع العقوبة (`timeout`, `kick`, `ban`, `none`) |

### 3. التشغيل المحلي

- **للتشغيل السريع:**
  ```bash
  cargo run
  ```
- **للبناء والتشغيل بأعلى أداء (Release):**
  ```bash
  cargo build --release
  ./target/release/zrx-protection-bot
  ```
- **على ويندوز:** بعد تنفيذ `cargo build --release` يمكنك تشغيل ملف `start.bat`.

---

## 🐳 التشغيل عبر Docker (24/7)

```bash
docker compose up -d --build
```

لمتابعة السجلات:
```bash
docker compose logs -f
```

---

## 🐧 التشغيل كخدمة على خادم Linux (VPS)

```bash
sudo apt update && sudo apt install -y build-essential pkg-config libssl-dev
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env
cargo build --release
```

أنشئ ملف خدمة systemd (`/etc/systemd/system/protection-bot.service`):

```ini
[Unit]
Description=Discord Protection Bot
After=network.target

[Service]
Type=simple
WorkingDirectory=/path/to/protection-bot
ExecStart=/path/to/protection-bot/target/release/zrx-protection-bot
Restart=always
RestartSec=5
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target
```

ثم فعّل الخدمة:
```bash
sudo systemctl daemon-reload
sudo systemctl enable --now protection-bot
```

---

## 🎮 الأوامر المتاحة

| الأمر | الصلاحية | الوصف |
| :--- | :--- | :--- |
| `/setup` | مالك السيرفر أو Whitelist | فتح لوحة التحكم التفاعلية لإدارة أنظمة الحماية |
| `/setwebhook url:<رابط>` | مالك السيرفر أو Whitelist | تعيين واختبار رابط Webhook لسجلات الحماية |
| `/unhack user:@عضو` | مالك السيرفر أو Whitelist | فك العزل عن عضو واسترجاع رتبه وصلاحيات القنوات |
| `/whitelist add role:@رتبة` | مالك السيرفر أو Whitelist | إضافة رتبة إلى القائمة البيضاء |
| `/whitelist remove role:@رتبة` | مالك السيرفر أو Whitelist | إزالة رتبة من القائمة البيضاء |
| `/whitelist list` | مالك السيرفر أو Whitelist | عرض الرتب المستثناة من الحماية |

---

## 📜 الترخيص

مشروع مفتوح المصدر بترخيص **MIT** — بواسطة **zrx** (قابل للتعديل والاستخدام الحر بالكامل).
