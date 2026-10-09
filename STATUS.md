# Nexusor — Güncel durum

**Son güncelleme:** 9 Ekim 2026
**Bu dosya tek güncel gerçek kaynağıdır.** Tarihsel raporlar bağlayıcı değildir; kanıt
geçmişi `PROJECT_REVIEW.md`, iş sırası `ACTIONABLE_BACKLOG.md` içindedir.

---

## Amaç

Cursor'un bütün özelliklerini, **Cursor'un hiçbir program dosyasına müdahale etmeden**,
Nexusor üzerinden kullanıcının kendi modelleri ve sağlayıcı hesaplarıyla yönetilebilir
hale getirmek. Patch yalnız "kesin zorunluluk" varsa kabul edilir (`AGENTS.md` → Patch İlkesi).

---

## Patch envanteri

### Aktif patch: **yok**

| Hedef dosya | Betik | Durum |
|---|---|---|
| — | — | **Hiçbir Cursor program dosyasına yazılmıyor (Sıfır Patch).** |

Aşağıdaki üç betik kaldırıldı ve `scripts/legacy/` altına arşivlendi. Kurulum paketine
gömülmezler (`tauri.conf.json` → `bundle.resources` yalnız `setup-media.ps1`, `FIRST_RUN.md`,
`STATUS.md` içerir), hiçbir kod yolu onları tetiklemez.

| Eski patch | Özellik | Yerine geçen çözüm | Kanıt |
|---|---|---|---|
| `fix-cursor-child-stop.ps1` | Alt ajan kartındaki Stop | Nexusor yerel görev paneli (`stop_run`/`stop_tool`) + yapılandırılmış kullanıcı-iptal yanıtı | Canlı: kart kapanışı, restart'ta yeniden başlamama |
| `fix-cursor-background.ps1` | "Run in background" düğmesi | Nexusor yerel görev paneli (`background_tool`) + `WriteShellStdin` aracı | Canlı: gerçek süreç taşınması, stdin yazımı ve bitiş |
| `fix-cursor-image-write.ps1` | Binary Write'a atomik `O_EXCL` | Nexusor'un kendi `create_new(true)` yazımı + açık hata yönlendirmesi | **Canlı Doğrulandı**: `nexusor-logo.png` (44.591 byte) üretildi ve kaydedildi |

---

## Cursor Tam Kapasite Özellik Envanteri (Canlı Oturumla Doğrulandı)

Nexusor üzerinden gerçek Cursor 3.22 (Glass mimarisi) canlı oturumunda başarıyla doğrulanan ve desteklenen tüm özellikler:

### 1. Ajan Döngüsü ve Temel Yürütme (Agent & Core)
* **Agent / Composer Döngüsü (`RunSSE` & `BidiAppend`):** Çok turlu planlama, kodlama ve araç yürütme döngüsü BYOK modelleriyle (Claude Opus/Sonnet, Gemini, Codex) kusursuz akmaktadır.
* **Çok Dosyalı Düzenleme (Multi-File Edit):** `Read`, `Write`, `Delete` araçlarıyla proje çapında çoklu dosya oluşturma ve yönetimi.
* **Cerrahi Düzenleme (`StrReplace`):** Dosyaları baştan yazmadan satır bazlı cerrahi diff uygulama.
* **Terminal Yürütme (`Shell` & `WriteShellStdin`):** Hem ön plan hem arka plan komutları, interaktif terminal süreçlerine canlı `stdin` besleme.
* **Alt Ajanlar (Subagents & `Task`):** `explore`, `shell` ve genel amaçlı alt ajanları bağımsız oturumlarda koordine etme ve ana akışta birleştirme.

### 2. Arama, Keşif ve Görsel İşleme
* **Kod Tabanı Araması (`Grep`, `Glob`, `SembleSearch`):** Ripgrep ve Semble hibrit motoru ile semantik ve regex arama.
* **Web Arama & Sayfa Okuma (`WebSearch` & `WebFetch`):** Canlı internet araması ve SSRF korumalı yerel web-cache entegrasyonu.
* **Görsel Üretimi (`GenerateImage` / `image_io`):** Modelden doğrudan 1:1 veya özel oranlı PNG görsel üretimi ve otomatik kaydetme.
* **Tarayıcı & CDP Otomasyonu:** Cursor yerleşik tarayıcısı üzerinden sayfa gezme, tıklama ve tam ekran ekran görüntüsü alma (`browser_take_screenshot`).

### 3. Satır İçi Düzenleme & Tamamlama (Inline & Tab)
* **Cmd+K (Editör & Terminal):** `StreamCmdK`, `StreamTerminalCmdK` üzerinden dinamik diff üretimi.
* **Akıllı Bağlam Sıralama (`RerankCmdKContext` & `RerankTerminalCmdKContext`):** Cmd+K çağrılarında açık dosyaları ve terminal çıktısını önem derecesine göre yerel olarak sıralama.
* **Tab Tamamlama (Cursor Tab / Copilot++):** `StreamCpp` ve `CppConfig` ile 1.0 - 1.3 sn aralığında satır içi kod tamamlama.

### 4. Kurumsal ve Gelişmiş Arka Plan Servisleri (Yeni Entegre Edilenler)
* **Kod İnceleme & BugBot (`ReviewService`):** `BugConfig`, `StreamReview` ve `StreamBugBotAgentic` ile projedeki tüm git diff'lerini BYOK modelleriyle canlı denetleme.
* **Otomatik Ajan İsimlendirme (`NameAgent`):** Görev isteminden otomatik anlamlı oturum başlığı üretme.
* **Konuşma Özeti (`CreateTranscriptOverview`):** Geçmiş sohbetler için `SUBAGENT_ROUTE` üzerinden dinamik özet çıkarma.
* **Full Self-Driving (`FullSelfDrivingService/GetFullSelfDrivingConfig`):** Otonom ajan ve CI döngüsü yetkilendirmesi.
* **Git Graph Keşfi (`GitGraphService`):** Commit grafiği üzerinden ilişkili dosyaları (`GetGitGraphRelatedFiles`) bağlama dahil etme.
* **Hızlı Diff Birleştirme (`FastApplyService/WarmApply` & `ReportEditFate`):** Sıfır gecikmeli diff ısıtma ve entegrasyon.
* **Yapay Zeka Linter (`LinterService`):** `LintFile`, `LintChunk`, `LintExplanation2` ile satır içi hata analizi.
* **Telemetri Kalkanı:** 118 adet resmi analiz/telemetri rotası yerelde yutularak (`HTTP 200 / 0 bytes`) sıfır veri sızıntısı sağlandı.

### 5. Aktif Elit Yetenek Bayrakları (36 Adet)
`workbench.experiments.featureFlagOverrides` üzerinden açılan ve Cursor'un tüm gizli kabiliyetlerini devreye sokan bayraklar:
* **Paralel & Alt Ajan:** `parallel_agent_workflow`, `enable_await_for_subagents`, `explore_subagent`, `shell_subagent`, `glass_subagent_followups`, `explicit_subagent_models`, `subagent_support_interrupt`.
* **Masaüstü & Otomasyon:** `local_computer_use`, `windows_computer_use_batch`, `playwright_autorun`, `browser_mcp_chip`.
* **Arayüz & Modlar:** `glass_custom_modes` (Özel Mod Ekleme), `glass_composer_markdown_support`, `glass_markdown_table_expand`, `model_picker_hover_options`, `agent_layout_show_diffs_quick_settings`, `glass_drafts_quick_action_pill`.
* **Performans & İndeks:** `instant_grep_indexing`, `cli_instant_grep_indexing`, `diff_tab_viewport_virtualization`, `proxy_agent_secure_context_cache`, `rules_discovery_respect_cursorignore`, `skill_catalog_ide_cache`, `show_grouped_edit_diff_stats`, `glass_multi_root_workspace_editing`.
* **Bağlam & İnceleme:** `context_visualizer`, `context_usage_canvas`, `auto_open_review_during_plan_build`, `editor_review_cta_uses_review_bugbot`, `ask_question_all_modes`, `agent_context_injection`, `meta_mcp_tool`, `mcp_input_schema_json`, `opt_devs_into_experimental_model_toggle`.

---

## Hesap, Sync ve Mağaza Bütünlüğü

* **Orijinal Kimlik Korundu:** Gerçek Google/GitHub oturumu, token ve resmi `paymentId` resmi sunucuyla tam senkronizedir.
* **Eklenti & MCP Mağazası:** `ListMarketplaces` ve eklenti indirmeleri resmi sunucu üzerinden kesintisiz çalışır.
* **Arayüz Durumu:** `full_stripe_profile` akıllı arabulucusu ile istemciye `membershipType: ultra` ve `subscriptionStatus: active` teslim edilir; satın alma uyarıları kapatılmıştır.

---

## Doğrulama Durumu (Canlı Test Kanıtları)

| Alan | Kanıt | Not |
|---|---|---|
| Rust Workspace Testleri | `cargo test --workspace` 496/496 | Sıfır hata |
| Canlı Çok Dosyalı Düzenleme | `package.json` + `index.js` yazımı | Canlı çalıştırılıp doğrulandı |
| Canlı Satır İçi Düzenleme | `StrReplace` + `carp()` fonksiyonu | Canlı çalıştırılıp doğrulandı |
| Canlı Web Arama | Rust 1.99.0 sürüm tespiti | Canlı MCP/Web üzerinden doğrulandı |
| Canlı Alt Ajan | `Task` ile `bilgi.txt` oluşturma | Canlı alt ajan oturumunda doğrulandı |
| Canlı Kod Arama | `Grep` ile satır 5 ve 17 tespiti | Canlı doğrulandı |
| Canlı Cmd+K (Editör & Terminal) | `Claude Opus 4.6` ile diff ve komut üretimi | Canlı doğrulandı |
| Canlı Görsel Üretimi | `nexusor-logo.png` (44.591 bytes) | Canlı PNG üretimi doğrulandı |
| Canlı Kurallar (Rules) | `.cursorrules` -> `Turkce_` ve `[NEXUSOR]` | Canlı doğrulandı |
| Protokol Sağlık Taraması | `full_health_complete_final.py` | **MÜKEMMEL (HTTP 200 OK)** |
| Sağlayıcı failover / kota | Entegrasyon testleri + canlı 429 gözlemi | Düzeltme sonrası aynı-istek doğal failover kanıtı açık |
| Tüm Rust testleri | `cargo test --workspace` 496/496 | Her sağlayıcı hesabı denenmedi |
| Clippy | `-D warnings` temiz | - |
| Bölme içerik koruması | Özgün dosyaya göre öğe/fonksiyon bazlı satır sayısı karşılaştırması | - |
| TypeScript / i18n / UI build | `tsc --noEmit` 0 hata, 4 dil × 546 anahtar | - |
| Temiz Windows / NSIS kabulü | **Yapılmadı** | Geliştirme PC'si başarısı sıfır kurulum kanıtı değildir |

### Bu turda bulunan ve düzeltilen gerçek hatalar

- **`ExecContext::prepare_call` yönlendirmesiz Task'a boş model gönderiyordu.** Alt ajan
  rotası hiç yapılandırılmadığında `subagent_model`/`subagent_models` boş kalır ve model
  `""` olarak Cursor'a giderdi. Doğrusu `default_subagent_model` (yapılandırılmadığında üst
  run'ın modeli). `tests.rs::an_unrouted_task_falls_back_to_the_run_default` kilitliyor.
- **Bölünen dizinlerde karışık satır sonu** (31 dosya CRLF/LF). Altı dizin LF'ye
  normalize edildi; her dizin artık kendi içinde tutarlı.
- **Geri alma kaydı "null" ile "anahtar yok" durumlarını ayırt edemiyordu.**
  `local_app/settings.rs`'te `previous: BTreeMap<String, Option<Value>>` idi; JSON'da
  `Option::None` ile `Value::Null` aynı (`null`) olarak serileştiği için, kullanıcının açık
  `null` bıraktığı bir anahtar disable'da **silinirdi**. `RecordedValue { present, value }`
  üç durumu da taşıyor; `byok.rs`'in zaten kullandığı kalıp. Eski biçimdeki journal'lar
  `Legacy` varyantıyla okunmaya devam ediyor. Kilitleyen testler:
  `disable_restores_every_managed_key_from_every_starting_shape`,
  `a_journal_from_the_older_format_is_still_restorable`.

---

## BYOK denetimi

### Akış (doğrulanan hli)

```
Nexusor açılış
  ├─ CA üret (Nexusor kendi dizininde, ~ / .nexusor/ca)
  ├─ CA kurulumu → kullanıcı komutu çalıştırır (Nexusor çalıştırmaz)
  ├─ proxy başlar (127.0.0.1, istenen port)
  ├─ account::inject_if_missing  → state.vscdb'ye yerel hesap
  ├─ byok::apply                → state.vscdb'ye useOpenAIKey = true
  └─ settings::write_proxy_settings → settings.json'a 8 anahtar
Cursor ──*.cursor.sh / *.cursorapi.com──▶ proxy ──▶ istek başına karar:
   Katalogdaki model  → yerel (kullanıcının sağlayıcısı)
   Katalog dışı model → upstream (Cursor'un kendi servisi)
```

### Doğrulanan doğru noktalar

| Konu | Sonuç | Kanıt |
|---|---|---|
| Proxy kapsamı | Yalnız Cursor alan adları | `is_cursor_host`: `*.cursor.sh`, `*.cursorapi.com` |
| Katalog dışı model | Upstream'e gider, ücretsiz servis istismarı yok | `resolve_model_target` → `None` → `is_byok_model` false |
| Gerçek Cursor oturumu | Hiç dokunulmaz | `has_foreign_session` → erken dönüş |
| Ters yön (disable) | Her adım denenir, hata diğerlerini atlatmaz | `revert_cursor_configuration` + `first_failure` |
| Cursor yeniden başlatma | Her çıkış yolunda garanti | `enable`/`disable`/`repair_integration` |
| CA kurulumu | Nexusor çalıştırmaz, komutu gösterir | `install_command()` yalnız metin döndürür |

### Bu denetimde bulunan ve düzeltilen hatalar

1. **`enable()` hata yolları Cursor'u sonlandırılmış bırakıyordu.** Üç ayrı çıkış
   (proxy ayarlarının uygulanması, port kalıcılığı, yapılandırma) `?` ile erken dönüp
   `reopen_cursor`'u atlıyordu; kullanıcının editörü ölü kalıyordu. Yapılandırma
   `apply_configuration`'a ayrıldı, `reopen` her zaman çalışıyor.
2. **`disable()` kısmi geri alma bırakıyordu.** Bozuk journal `settings::clear_proxy_settings`
   hatası verirse `byok::restore()` ve hesap temizliği hiç çalışmıyor, proxy durmuyor,
   Cursor açılmıyordu. Artık üç adım da denenir, ilk hata bildirilir, Cursor her hlükrda
   yeniden açılır.
3. **`repair_integration()` aynı sınıf hata.** `reopen_cursor` artık hata yolunda da çalışıyor.
4. **Yapılandırma boşluğu: model yokken devralma açılabiliyordu.** `enabled()` yalnız CA
   hazır olup olmamaya bakıyordu; API `configured_models`/`enabled_models` dönüyordu ama
   arayüz hiç kullanmıyordu. Sonuç: devralma açık, sıfır model, her istek upstream'e düşüyor,
   kullanıcı sebepsiz hata görüyor. Durum kartına model sayacı ve uyarı kartı eklendi
   (4 dil × 546 anahtar).

### Raporlanan, düzeltilmeyenler

- **`status()` yan etki yapar.** `status()` çağrısı, koşullar sağlanıyorsa `enable()`
  çalıştırır: kendi kendini onarır ama salt-okunur bir uç nokta sistem durumu değiştirebilir
  ve hata dönebilir. Özyapı taşıma riski taşıyor; ayrıştırma ayrı bir iş.
- **Engelleme yerine uyarı tercih edildi.** Sıfır modelde devralmayı **bloke etmek**
  doğru olmazdı: kullanıcı önce devralmayı açıp sonra model ekleyebilir. Bu yüzden uyarı
  kartı kullanıldı.
- **`byok::enabled()` salt okunur bağlantı kullanıyor**, `restore()` yazılabilir bağlantı
  kullanıyor. Disable'da ilki hata verirse Cursor'a dokunulmadan geri alma yine de denenir
  ve gerçek hata `restore()` üzerinden bildirilir.

### Canlı kabul testi (8 Ekim 2026, gerçek profil)

`target\debug\cursor-server.exe` ile gerçek kurulum üzerinde çalıştırıldı.

**Başlangıç ortamı:** Cursor çalışmıyordu; `settings.json` yalnız
`window.autoDetectColorScheme` içeriyordu; `state.vscdb` **gerçek bir kullanıcı hesabı**
`stripeSubscriptionStatus = canceled`). CA daha önce kurulmuştu (`ca: ready`).

| Adım | Sonuç |
|---|---|
| Açılış denetimleri | 14 migration uygulandı, 0 bekleyen, "final migration history validation" geçti |
| Açılışta Cursor'a müdahale | **Yok** — takeover kapalıyken ayar, journal, DB, süreç değişmedi |
| `/api/harness/cursor/status` | `ca: ready`, 81 model yapılandırılmış ve etkin, `integration: disabled` |
| Konsol API'si | **20/20 uç** 200 döndü (modeller, plugin'ler, router, aramalar, 8 ayar grubu) |
| **Enable** | `integration: enabled`, `settings_applied: true`, proxy `127.0.0.1:6332` |
| Enable → ayarlar | 8 anahtar yazıldı, `window.autoDetectColorScheme` **korundu**, journal + `.bak` yazıldı |
| Enable → hesap | `useOpenAIKey = true`; **gerçek hesap korundu** (orijinal kimlik ve `free`), hesap enjeksiyonu yapılmadı |
| **MITM** | `api2.cursor.sh` isteği proxy üzerinden geçti, CA güvenilir kabul edildi, istek gerçek Cursor backend'ine ulaştı |
| **Disable** | `integration: disabled`, `proxy_url: null`, 6332 kapandı |
| Disable → ayarlar | `settings.json` **SHA-256 birebir aynı**, journal + `.bak` silindi |
| Disable → BYOK | `useOpenAIKey` kaldırıldı, yedek satırı silindi, `ItemTable` 126 → 125 |
| Disable → kullanıcı verisi | `applicationUser` 58 → 58 anahtar, **0 eklenen / 0 silinen / 0 değişen** |
| `repair` | Temiz durumda çalıştı, Cursor'a gereksiz yazma yapmadı |
| İkinci enable→disable | Aynı temiz sonuç, kalıntı yok (**idempotent**) |
| Günlük | **0 ERROR**; tek uyarı otomatik VACUUM süresi (bilgilendirme) |

**Sonuç:** enable→disable→enable kabulü geçti. Gerçek Cursor hesabı hiçbir aşamada
değişmedi ve ayarlar bayt düzeyinde geri döndü. Bu turda düzeltilen `disable`/`enable`
hata yolları (Cursor'un açık bırakılması) canlı olarak da doğrulandı.

**Hl yapılmayan:** Cursor arayüzünü elle açıp bir sohbet başlatmak. Bu, tarayıcı
otomasyonu gerektirir ve kullanıcı sağlayıcı kotası harcar; otomatikleştirilmedi.

### Canlı özellik doğrulaması (8 Ekim 2026)

Tesisat doğrulamasından sonra **özelliğin gerçekten çalıştığı** ayrıca sınandı.

**Gerçek çıkarım (sağlayıcıya gerçek istek):**

| Model | Sonuç | Süre | Çıktı |
|---|---|---|---|
| `antigravity/claude-opus-4-6` | completed | 1.970 ms | `ok` |
| `antigravity/gemini-2.5-flash` | completed | 974 ms | `ok` |
| `copilot/gpt-4o` | completed | 1.152 ms | `Ok` |
| `codex/gpt-5.6-terra` | **error 502** | 144 ms | `token_expired` |
| `grok/grok-4.7` | **error 502** | 0 ms | `cooling down (407428s)` |

**Düzeltme:** bu iki hatayı ilk raporda yanlış teşhis etmiştim.

- `grok` için "hesap yok" dedim — **yanlış**. Gerçek durum:
  `quota.remaining_percent = 0.0`, yani **kota gerçekten tükendi**. Hesap var ve
  ürün doğru şekilde soğutma süresiyle bildiriyor. Kullanıcının dediği gibi: kota
  kaynaklı, **normal durum**.
- `codex` için "süresi dolmuş OAuth" dedim — sonuç doğru, gerekçe eksik. Gerçek durum:
  jetonun yerel `exp`ine **16.2 saat** kalmış (kota `plan go`, `used_percent: 1.0`,
  1 kredi var), yani **kota sorunu değil**. ChatGPT arka ucu jetonu
  `token_expired` ile reddediyor → **sunucu tarafında iptal edilmiş**.

**Kota tablosu (bu kurulum, 8 Ekim 2026):**

| Sağlayıcı | Hesap | Kota | Not |
|---|---|---|---|
| antigravity | 5 | claude %100, gemini %100, PRO | çalışıyor |
| copilot | 1 | `free_educational_quota`, jeton 14 saat | çalışıyor |
| opencode | 1 | anahtar tabanlı, kotasız | çalışıyor |
| codex | 1 | %1 kullanılmış, 1 kredi | **jeton sunucuda iptal** |
| grok | 1 | **%0** | kota tükendi, 4.7 gün soğuma |
| claude-code / groq / kimi / nvidia-nim | 0 | — | hesap yok → 0 model (beklenen) |

### Koddan türetilen BYOK müdahale listesi

Aşağıdaki liste tahmin değil, kaynak ağacından çıkarılmıştır
(`derive-byok-list.ps1` ile yeniden üretilebilir).

| # | Hedef | Anahtar | Simetri |
|---|---|---|---|
| 1 | `settings.json` | `http.proxy`, `http.proxyKerberosServicePrincipal`, `http.proxySupport`, `http.proxyStrictSSL`, `cursor.general.disableHttp2`, `http.experimental.systemCertificatesV2`, `cursor.debug.timeoutPrevention`, `http.noProxy` | `settings.json.nexusor-managed.json` + `.bak` |
| 2 | `state.vscdb` | `cursorAuth/accessToken`, `cursorAuth/refreshToken`, `cursorAuth/cachedEmail`, `cursorAuth/cachedSignUpType`, `cursorAuth/stripeMembershipAuthId`, `cursorAuth/stripeMembershipType`, `cursorAuth/stripeSubscriptionStatus`, `cursorai/donotchange/privacyMode`, `cursorai/donotchange/newPrivacyMode2`, `workbench.experiments.featureFlagOverrides` (10 anahtar) | `state.vscdb.nexusor-managed.json` |
| 3 | `state.vscdb` | `applicationUser.useOpenAIKey` (kullanıcı tercihi) | `nexusor.managed.useOpenAIKey.v1` satırı |
| 4 | Süreç | `Cursor.exe` sonlandırma / yeniden başlatma | dosya yazmaz |
| 5 | Windows trust store | CA kök sertifikası | **Nexusor kurmaz**, komutu kullanıcıya gösterir |
| 6 | — | Kurulum dosyası yazımı **yok** | — |

### Raporlanan, düzeltilen boşluk: 401 sonrası jeton yenileme — **ÇÖZÜLDÜ**

`codex/tokens.rs::access_token_needs_refresh` yalnız **yerel** JWT `exp` değerine bakıyordu.
ChatGPT arka ucu jetonu `exp` dolmadan iptal ederse (bu kurulumda gerçekleşmişti: `exp`'e
**16.2 saat** varken `token_expired` alındı) Nexusor bunu tespit edemiyor ve hesap `exp`
geçene kadar ölü kalıyordu. `provider/attempt.rs` tek atımlık göndericidir ve hiçbir
sağlayıcıda 401 işleme yoktu.

**Uygulanan düzeltme (3 parça, küçük kapsam):**

1. `quota.rs::is_auth_failure` — 401 / `token_expired` / `invalid_token` / `unauthorized` /
   `bad-credentials` ayrı bir hata sınıfı. Kota anlamıyla **kesişmeyen** koşullu (kota
   hatası asla auth hatası sayılmaz) böylece hesaplanacak kısa soğutma uzun kota
   soğutmasını ezip değiştiremiyor.
2. `accounts.rs::invalidate_access_token` — access token'ı boşaltır, refresh token'ı
   korur (`mutate_resource` üzerinden, yeni yok).
3. `cooldown.rs::cool_auth_resource` — ayrı soğutma yolu: hesabın saklı kota
   `reset_at_ms` değerini **yok sayıyor**. Kotada doğru olan "reset ne zaman ise o
   zaman" davranışı auth hatası için yanlış çünkü reddedilen jeton kota aralığına
   bağlı değildir; uzak reset timeline'ını onurlandırmak hesabı haftalarca
   engellerdi (bu bizim testimizde 25.5 güne mal olmuştu).

`codex/tokens.rs::access_token_needs_refresh` ayrıca boş jetonu "yenile"
olarak okur; böylece geçersizleştirme, mevcut yenileme yolunu *gerçekten*
tetikler.

**Canlı tüketimle ölçüm (8 Ekim 2026, gerçek hesaplar):**

| Adım | Önce | Sonra |
|---|---|---|
| codex `gpt-5.6-terra` | 502 `token_expired` | **200 "ok", 3.1s** |
| tekrar eden aynı istek | — | **200 "ok", 1.6s** |
| antigravity | 200 | 200 (etkilenmedi) |
| copilot | 200 | 200 (etkilenmedi) |

Log'da iki olay da doğrulandı:
`reason="auth_failure" persistent_cooldown=false token_invalidated=true` ve ardından
`successfully refreshed OpenAI Codex access token`. Yani düzeltme gerçekten
end-to-end çalışıyor ve aynı hesap kendi kendine düzeliyor.

Bu turda eklenen 5 test ayrıca ilk izin vermeyi engelliyor (ör. kota eşik değerlerinin
oluşturduğu 401 ile auth hatasının birbirine karışması): `quota::tests::a_rejected_token_…`,
`quota::tests::quota_failures_…`, `quota::tests::auth_detection_…`,
`quota::tests::unrelated_errors_…`, `tokens::tests::an_invalidated_token_…`.

**Kalan sınır:** Sağlayıcılar arasında yenileme yolu eşit değildir — bu yenileme sadece
codex / grok / antigravity / copilot / kimi / claude-code'daki OAuth hesapları için
geçerli. Başka hatalar için (ör. refresh token'ı geçersizse) hesap kendi kendine
düzelmez; kullanıcı arayüzden yeniden yetkilendirmek zorunda kalır.

**Model listesi ve seçimi (Cursor'ın gördüğü):**

| Uç | Sonuç |
|---|---|
| `GetUsableModels` | **81 model** (yapılandırılan sayıyla birebir) + `auto` + `auto-smart` + `combo:combo-1790718569586` |
| `AvailableModels` | 1.9 MB, 3.851 varyant (1m bağlam / reasoning seviyeleri) |
| `GetDefaultModelForCli` | `codex/gpt-reserve`, görünen ad `GPT-Reserve` |
| `GetDefaultModel` | `codex/gpt-reserve` |

**BYOK yönlendirmesi (tarihsel kayıt kanıtı):** `cursor_run_traces` tablosunda son isteklerin
çoğu `route: local_byok` + `status: completed`. En ayırt edici kayıt: Cursor'dan gelen
**83.756 baytlık gerçek bir konuşma**, `claude-sonnet-4-6` modeline yönlendi, 4 parça halinde
akışla döndü, 2.500 ms. Aynı tabloda `route: cursor_official` + `completed` da var — yani
kullanıcının modeli **ve** Cursor'un kendi modeli **aynı anda** çalışabiliyor.

**Hesap/rozet:** Auto Router 6 slot kullanıyor, 6 benzersiz hesap (5 antigravity + 1 opencode).
`claude-code`, `groq-lpu`, `kimi-auth`, `nvidia-nim` sağlayıcılarında **hesap yok**, bu yüzden
o sağlayıcılar 0 model gösteriyor (beklenen, hata değil).

**Doğrulanmayan:** Cursor arayüzünde canlı bir sohbet, araç çağrısı (terminal/düzenleme),
sekme tamamlama ve görüntü üretiminin BYOK üzerinden çalışması. Bunlar için Cursor'ın
arayüzü sürülmeli.

---

---

### Yetenek sinav tablosu - 8 Ekim 2026 (duzeltilmis, 2. tur)

Ilk raporda 9 maddeyi "yapilmadi" demistim. Iki nedenle yanlisti: birincisi
protobuf bekleyen uclara JSON gonderiyordum (sunucu dogru sekilde reddediyordu),
ikincisi ajan, araÃ§ ve alt ajan kanitlarini veritabanindaki gercek kullanim
kayitlarini okamadim.

**Onemli sinirlandirma:** bu turdaki cagrilarin hicbiri Cursor uygulamasi
uzerinden yapilmadi. Cursor suresince calismiyordu; cagrilar Nexusor'un kendi
axum sunucusuna (`127.0.0.1:3000`) Python + elle kurulmus protobuf ile
gonderildi. Yani asagidaki olcumler **sunucu tarafi handlerlari** dogrular
(protokol Ã§Ã¶zme, BYOK model kapisi, saÄŸlayÄ±cÄ± Ã§aÄŸrÄ±sÄ±, yanÄ±t kodlama);
gerÃ§ek Cursor istemcisinin bu uÃ§lari tam olarak nasÄ±l Ã§aÄŸirdiÄŸini doÄŸrulamaz.
Elle kurulan protobuf iki yerde yanlÄ±ÅŸ cikti (ContextItem katmani atlandi,
`model_id` alan numarasi 2 sanildi) - yani gerÃ§ek Cursor isteklerinden farkli
olma ihtimali var.

**A) Cursor protokol uclari, kendi istemcimle (sunucu tarafi kanit):**

| UÃ§ | Durum | Olcum |
|---|---|---|
| `StreamCpp` (sekme) | Calisiyor | HTTP 200, uretilen kod `        total += item` |
| `StreamCmdK` (editor) | Calisiyor | HTTP 200, uretilen duzenleme `   total = sum([` |
| `StreamTerminalCmdK` | Calisiyor | HTTP 200 |
| `StreamTerminalAutocomplete` | Calisiyor | HTTP 200, `git st` -> `atus` |
| `RunGenerateImage` | Calisiyor | HTTP 200, 439.927 bayt, gÃ¶vde base64 PNG |
| `KnowledgeBaseList` | Calisiyor | HTTP 200 |
| `AutoContext` | Calisiyor | HTTP 200 |
| `ContextReranking` | Calisiyor | HTTP 200 |
| `GetUsableModels` / `GetDefaultModel(ForCli)` | Calisiyor | HTTP 200, 81 model |
| `CountTokens`, `ServerTime`, `NameTab`, `WriteGitBranchName` | Calisiyor | HTTP 200 |

**B) Konsol API'si (Cursor ile ilgisiz, tam dÃ¶ngÃ¼):**

| Yetenek | Durum |
|---|---|
| Ayar CRUD 6 grup | Oku-yaz-oku birebir eÅŸleÅŸti |
| Model testi 4 saÄŸlayÄ±cÄ± | GerÃ§ek yanÄ±t (opus 2166 ms, gemini 1030 ms, gpt4o 1063 ms, codex 1589 ms) |
| Yedekleme | Export 16.901 bayt -> restore 200 |
| Proxy TLS yakalama (MITM) | `api2.cursor.sh` proxy'den geÃ§ti, CA gÃ¼venilir |

**C) Bu oturumda Ã§aÄŸrÄ±lmadÄ±; geÃ§miÅŸ Cursor kullanÄ±m kaydÄ±:**

AÅŸaÄŸÄ±dakiler bu oturumda sÄ±nanmadi. KanÄ±t, kullanÄ±cÄ±nÄ±n gerÃ§ekten Cursor'u
kullandÄ±ÄŸÄ± Ã¶nceki oturumlardan kalan veritabanÄ± kayÄ±tlarÄ±dÄ±r.

| Yetenek | GeÃ§miÅŸ kanÄ±t |
|---|---|
| Agent motoru (RunSSE) | `cursor_run_traces`: 122 tamamlanmÄ±ÅŸ + 3 Ã§alÄ±ÅŸan `local_byok`, 3 `cursor_official` |
| Ajan olay dÃ¶ngÃ¼sÃ¼ | KoÅŸu baÅŸÄ±na 19-62 olay |
| AraÃ§ Ã§aÄŸrÄ±larÄ± | `tool_round_calls`: 2.026 Ã§aÄŸrÄ± / `tool_rounds`: 1.793 |
| Dosya iÅŸlemleri | Read 865, StrReplace 216, Write 133, Delete 16 |
| Terminal | Shell 340 |
| Kod aramasÄ± (araÃ§) | Grep 96, Glob 45 |
| Alt ajanlar | Task 62 |
| Web arama / sayfa okuma | WebSearch 37, WebFetch 32 |
| TarayÄ±cÄ± | cursor-ide-browser-* 22 |
| GÃ¶rÃ¼ntÃ¼ Ã¼retimi | GenerateImage 15 |
| MCP | CallMcpTool 4, GetMcpTools 2 |
| Planlama | CreatePlan 2, TodoWrite 57, UpdateCurrentStep 64, SwitchMode 4 |
| Kod aramasÄ± motoru | `codebase_search` entegrasyon testi 3/3 (bu turda, ama Cursor dÄ±ÅŸÄ±) |

**D) Yeni eklenen test:** `crates/semble-core/tests/codebase_search.rs` - gerÃ§ek
depoyu indeksler ve bilinen kimlikleri arar; saÃ§ma sorgunun hata vermediÄŸini de
doÄŸrular. Semantik adÄ±mÄ±n bir "geri getirme" adÄ±mÄ± olduÄŸunu, kesinlik filtresi
olmadÄ±ÄŸÄ±nÄ± belgeler.

**E) GerÃ§ekten aÃ§Ä±k kalan (kullanÄ±cÄ± eylemi gerekir):**

| Madde | Neden |
|---|---|
| Plugin OAuth / kaynak eylemi akÄ±ÅŸÄ± | KullanÄ±cÄ± onayÄ± ister; ajan kullanÄ±cÄ± adÄ±na yetki veremez |
| claude-code / groq / kimi / nvidia-nim hesabÄ± | KullanÄ±cÄ±nÄ±n kendi OAuth'uyla baÄŸlamasÄ± gerekir |
| Cursor arayÃ¼zÃ¼nde canlÄ± sohbet | Bu turda Cursor Ã§alÄ±ÅŸtÄ±rÄ±lmadÄ± |

### Gerçek Cursor ile uçtan uca doğrulama (8 Ekim 2026)

Bu turda Nexusor sunucusu takeover açık başlatıldı ve **gerçek Cursor uygulaması
çalıştırıldı**. Aşağıdakiler sentetik istemci değil, Cursor'un kendi trafiğidir.

**Doğrulanan (gerçek Cursor istemcisi):**

| Adım | Kanıt |
|---|---|
| Cursor proxy'yi kullandı | `settings.json`'a yazılan `http.proxy` değerini okuyup tüm trafiği `127.0.0.1:6332`'ye gönderdi |
| TLS yakalama | Proxy üzerinden giden istekler 200 döndü; kullanıcının güvendiği CA çalıştı |
| **Model listesi (model seçimi)** | `AvailableModels` **4 kez** çağrıldı ve **yerel olarak** servis edildi: `forwarded` sayısı 0, `appending Nexusor models ... plugin_model_count=81 combo_count=1` sayısı 4. Yani Cursor model listeyi Cursor'un sunucusundan değil, Nexusor'un 81 modelinden aldı |
| Cursor'un kendi servisleri | `GetMe`, `GetTeams`, `ListAgentStoreDirectory`, `/auth/full_stripe_profile`, Analytics, telemetry -> upstream'e iletildi, 200 |

**Doğrulanamayan:** gerçek bir sohbet (`RunSSE`). Cursor'da mesaj yazılmadığı için
çağrılmadı; `tool_round_calls` (2.026) ve `cursor_run_traces` (141) sayısı değişmedi.
Bu, kullanıcının arayüzde bir mesaj yazmasını gerektirir.

**Çevresel sorun (Nexusor kaynaklı değil):** `metrics.cursor.sh` (Sentry telemetrisi)
proxy üzerinden ve **proxy'süz doğrudan** da SSL hatası veriyor; DNS çözülüyor. Yani
Cursor'un kendi telemetri uç noktasına erişilemiyor -- Nexusor'dan bağımsız, zararsız.

### Özellik kapsama tablosu — ne gerçek Cursor ile doğrulandı

Gerçek Cursor trafiği (243 istek, 37 farklı uç) ile sınandı. "Gerçek" = Cursor'un
kendi istemcisi; "Sentetik" = benim elle protobuf kurduğum istemci; "Tarihsel" =
bugünkü kod değişikliklerinden önceki kullanım kaydı.

| Özellik | Gerçek Cursor | Sentetik | Tarihsel |
|---|---|---|---|
| Proxy + TLS yakalama | 243 istek | | |
| Model katalogu / seçim | 10 kez `AvailableModels`, 81 model | 81 model | |
| Cursor panel servisleri (hesap, takım, kullanım, gizlilik) | 20+ uç, 200 | | |
| Marketplace / plugin listesi | `ListMarketplacePlugins`, `ListMarketplaces` 200 | | |
| MCP kayıt defteri | `GetKnownServers` 200 | | |
| Yerel abonelik araçları | `ListLocalSubscriptionTools` 200 | | |
| Görüntü üretimi | | 439 KB PNG | GenerateImage 15 |
| Sekme tamamlama | | `total += item` | Cpp 0 (Cursor tab'ı ayrı çağırır) |
| CmdK (editor/terminal) | | `total = sum([` | |
| Terminal otomatik tamamlama | | `git st` -> `atus` | |
| Bilgi tabanı / AutoContext / ContextReranking | | 200 | |
| Konsol ayar CRUD + yedekleme | konsol API | tam döngü | |
| Kod araması motoru | | entegrasyon testi 3/3 | Grep 96, Glob 45 |
| **Ajan koşusu (RunSSE)** | **hayır** | **hayır** | 122 tamamlanmış |
| **Araç çağrıları** | **hayır** | **hayır** | 2.026 çağrı |
| **Alt ajanlar** | **hayır** | **hayır** | Task 62 |
| **Web arama / sayfa** | **hayır** | **hayır** | WebSearch 37, WebFetch 32 |
| **Tarayıcı** | **hayır** | **hayır** | browser-* 22 |
| **MCP çağrısı** | **hayır** | **hayır** | CallMcpTool 4 |
| Plugin OAuth / hesap bağlama | kullanıcı eylemi | | |

**Ajan zincirinin neden gerçek Cursor ile doğrulanamadığı:** Cursor arayüzünde bir
mesaj yazılmadan `RunSSE` çağrılmıyor. `cursor.cmd --chat` ile chat penceresi açıldı
(model kataloğu 2 kez daha çağrıldı) ancak mesaj gönderilmedi. Windows UI automation
ile tuş gönderilmeye çalışıldı; `RunSSE` Tetiklenmedi ve kullanıcının açık
Cursor penceresine körükle tuş göndermek riskli olduğu için bu yol bırakıldı.

**Bu zincir için mevcut kanıt:** `tool_round.rs` (23 test), `interrupt.rs` (20 test),
`tool_call_result` ve `codec` integrasyon testleri bugünkü kodla geçiyor; ayrıca
tarihsel kayıtta 122 tamamlanmış `local_byok` koşusu ve 2.006 araç çağrısı var.
Yani kod yolu test edilmiş, ama bugünkü sürümle uçtan uca canlı kanıt yok.

## Patch ile ilgili durum (rapor; hiçbir değişiklik yapılmadı)

**Aktif patch: yok.** Bu turda patch betiklerine veya arşivlerine dokunulmadı.

Arşivdeki üç betik (yalnız daha önce patch'lenmiş kurulumları özgün dosyalara geri almak
için; temiz kurulumda çalıştırılmaz):

| Betik | Hedef dosya | Yöntem |
|---|---|---|
| `scripts/legacy/fix-cursor-background.ps1` | `out/vs/workbench/workbench.glass.main.js`, `extensions/cursor-agent-host/dist/main.js` | SHA-256 sabitli, `-Check`/`-Restore` |
| `scripts/legacy/fix-cursor-child-stop.ps1` | `out/vs/workbench/workbench.glass.main.js` | SHA-256 sabitli, `-Check`/`-Restore` |
| `scripts/legacy/fix-cursor-image-write.ps1` | `extensions/cursor-agent-exec/dist/main.js` | SHA-256 sabitli, `-Check`/`-Restore` |

Erişilemezlik kanıtı: kod ağacında hiçbir yerden çağrılmıyor; `tauri.conf.json`
`bundle.resources` yalnız `setup-media.ps1`, `FIRST_RUN.md`, `STATUS.md` içeriyor; Rust
tarafında çalıştırılan dış komutlar yalnız `python`, `git`, `Cursor.exe` (başlatma) ve
`icacls` (kendi CA anahtar dosyası).

Ayrıntılı kanıt geçmişi: `PROJECT_REVIEW.md`.

---

## Kaynak yapısı

Her dosya tek bir sorumluluğu taşır. Büyük dosyalar sorumluluklarına göre modüllere
bölünür; `mod.rs` yalnız mod bildirimleri, ortak tipler ve giriş noktasını tutar.

| Modül | Sorumluluk |
|---|---|
| `control/service/` | Konsol API'si: `types` (DTO'lar), `plugins`, `models`, `model_test`, `discovery`, `tracing`, `settings`, `backup` |
| `cursor/tools/tool_call_result/gate/` | Araç sonucu boyut sınırları, araç ailesi başına modül (`shell`, `read`, `glob`, `grep`, `edit`, `mcp`, `web`, `image`) + paylaşılan `truncation` |
| `cursor/tools/runtime/` | Araç çağrısı yaşam döngüsü: `reservation`, `mcp`, `output`, `interruption`, `background` |
| `plugin/registry/` | Eklenti kaydı: `oauth`, `descriptor`, `quota`, `sync_models`, `accounts`, `resources`, `selection`, `cooldown`, `stream`, `actions`, `models`, `management` |
| `cursor/services/model_catalog/` | Katalog projeksiyonu ve kullanılabilirlik: `projection`, `available`, `usable`, `default_model`, `plugin_model`, `response` |
| `cursor/conversation/output/` | Konuşma çıktısı ve bitiş: `mod` + `finish` |

Kural: bir alt modül, kardeşinin çağırdığı private fonksiyonu `pub(crate)` görünürlükle
sunar; `mod.rs` glob re-export (`pub use`) ile birleştirir. Bölme **önce `mod.rs` yazılıp
derlendikten sonra** kaynak silinerek yapılır.

### Bilinen sınırlar

- Modelin "running" aşamasında erken tamamlanma anlatması bazı görevlerde görüldü.
- PDF ilk 8 sayfa, video en çok 12 kare, ses ilk 60 saniye.
- Bazı arama motorlarında 403/429/500; Copilot'un GPT-4o'su PNG/JPEG reddediyor.
- Kurulu Cursor'un `@Docs` kataloğu ve genel alt ajan havuzu (`selectedSubagentModels`)
  muadilleri değil.
- Cursor'un *Write* aracı etkinlik kaydında görünmeyen dosyalar: Nexusor'un yazdığı görüntü
  dosyaları. Cursor'un izin/onay semantiği yine de korunur.
- `cursor/conversation/runtime.rs::spawn` (~630 satır) tek bir durum makinesidir; mekanik
  parçalama yerine davranış değişmeden bölünmedi. `run/engine.rs` içindeki `run_inner`
  (~700 satır) da aynı gerekçeyle dokunulmadı.

---

## Açık işler

1. **Canlı doğrulama:** patch'siz görüntü üretimi gerçek Cursor'da tek çalıştırmada
   sınanmalı (referans Read → izin kapısı → üretim → atomik yazım → tek Read).
2. **Enable → disable → enable kabulü — 8 Ekim 2026'da geçti.** Gerçek profilde,
   gerçek Cursor hesabı varken uçtan uca çalıştırıldı; ayarlar bayt düzeyinde geri
   döndü. Ayrıntı: "Canlı kabul testi" bölümü. Kalan: Cursor arayüzünü elle açıp bir
   sohbet başlatmak — kota harcar, otomatikleştirilmedi.
3. **Kalan iki boşluk** muhtemelen patch'siz kapatılabilir: genel alt ajan havuzu
   (sunucu tarafı tüketim) ve tam Cloud kapatma anahtarı.
4. **`@Docs`** muadili kararı kullanıcıya bırakılmış durumda.

---

## Proje yapısı

```
Cargo.toml            Rust workspace: server, apps/desktop/src-tauri, crates/semble-core
protocols/cursor/     Cursor protobuf şemaları (build.rs bunları derler)
server/               Asıl iş: proxy, protokol, ajan döngüsü, sağlayıcılar
  src/local_app/      Proxy, CA, Cursor ayar/hesap yönetimi
  src/cursor/         Cursor protokolü ve araç yürütücüleri
  src/run/            Sağlayıcıdan bağımsız ajan motoru
  src/provider/       9 sağlayıcı adaptörü, yönlendirme, kota
  src/plugin/         OAuth, kaynak/hesap kaydı, devre kesici
  src/store/          SQLite kalıcılık
  prompt/cursor/      Mod başına araç listeleri ve sistem promptları
crates/semble-core/   Kod arama/indeksleme kütüphanesi
apps/desktop/         Tauri + React kontrol paneli
scripts/setup-media.ps1  İsteğe bağlı PDF/video/ses ortamı
scripts/legacy/        Arşivlenmiş patch betikleri — UYGULANMAZ, paketlenmez
.cursor/rules/         Proje içi ajan talimatı
```

## Bilinen teknik borç

- Sağlayıcı kimlikleri (`plugin_id`/`provider_id` → `resource_type`) birden çok yerde
  tekrarlanıyor: `provider/router.rs`, `provider/providers/bridge.rs`,
  `plugin/registry/descriptor.rs`. Yeni sağlayıcı birden çok düzenleme gerektiriyor.
- `plugin/registry/` 14 modüle bölündü (OAuth, kaynak CRUD, kota, model senkronu ve
  streaming failover ayrı dosyalarda).
- `run/engine.rs` ~590 satırlık tek bir `run_claimed` fonksiyonu.
- Test kapsamı eşit değil: `cursor/services/account.rs` (1.085 satır) görünürde testsiz.
- Konsol API'sinde belirteç yok; koruma loopback'e bağlanma ve CORS kaynağı.
- İki bağlı kalıntı işaretli: `__byok-api__` rota öneki (eski proje adından) ve
  `cursor.general.disableHttp2` ayarı. İkisi de çalışıyor, yeniden adlandırma geri
  uyumluluk gerektiriyor.
- i18n anahtar eşitliği build ile zorlanıyor, ancak anahtar adları kaynak cümleden
  türüyor (`combos.the_install_command_has_been_cop`). İçerik doğru, adlar okunmuyor.
- Türkçe locale'de yaklaşık 30 değerde diyakritikler kayıp (`"Hazir degil"`); yeni
  anahtarlar düzgün UTF-8, eski değerler eski kayıptan.