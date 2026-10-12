# Nexusor — Cursor Yetenek Denetimi (Cursor 3.24.12)

**Tarih:** 12 Ekim 2026
**Kapsam:** Kurulu Cursor 3.24.12 kurulumunun tamamı, Nexusor kaynak kodunun tamamı
**Yöntem:** Her dosya kapsayıcı olarak taranmıştır. Örneklem veya tahmin yoktur.

Bu dosya bir **denetim kaydıdır**, iş emri değildir. Her bulgu kanıtıyla birlikte
verilmiştir. Doğrulanmayan hiçbir şey "doğrulandı" olarak işaretlenmemiştir.

---

## 1. Tarama kapsamı

| Ölçüm | Değer |
|---|---|
| Kurulumdaki toplam dosya | 15.077 (598 MB) |
| Cursor'a özgü dosya (taranan) | 108 |
| Taranan JS uzantı dosyası (node_modules hariç) | 233 |
| RPC yüzeyi | 2.282 metot / 85 servis |
| Kayıtlı araç (`hr()`) | 52 araç kimliği |
| Feature gate tablosu | 805 |
| Doğrudan okunan feature gate | 203 |

### Kapsam dışı bırakılanlar ve gerekçesi

- `node_modules/**` (13.364 dosya): stok npm paketleri. Cursor özelliği tanımlamaz.
- VS Code dil eklentileri (`typescript`, `python`, `rust` vb.): yukarı akış VS Code.
- Bunların dışındaki **tüm Cursor yüzeyi** taranmıştır.

### Tarama hatası kaydı

İlk taramada araç kimliği regex'i `{3,40}` ile sınırlıydı ve `LS` (2 karakter)
kaçtı. Düzeltilip tüm dosyalar yeniden tarandı; sonuç 51 → **52** araç.
Bu kayıt, gelecekte tarama yapılırken aynı hatanın tekrarlanmaması içindir.

---

## 2. Doğrulanmış doğru olanlar

Nexor'un temel tasarımı Cursor'un gerçek protokolüyle örtüşüyor. Bunlar **test edilerek**
doğrulandı, tahmin değil.

### 2.1 Müdahale noktası doğru

`out/vs/workbench/workbench.glass.main.js` → `buildFlags.js`:

```js
kl = { extensionIsDev:!1, developmentTooling:!1, enableTraceSpanCollection:!0,
       enableEmbeddingsModelToggle:!1, enableCPPControlTokenToggle:!1,
       cursorPredictionOptions:!1, localMode:!1 }
```

`localMode:!1` → yerel ajan ağ yolundan, `backendUrl` üzerinden gider. Yani
Nexusor'un proxy müdahalesi doğru yerde.

### 2.2 Çekirdek akış doğru

`extensions/cursor-agent-host/dist/main.js`:

- Giriş: `BidiTransport` → `this.bidiClient.bidiAppend({requestId, appendSeqno, data| dataBinary})`
- Çıkış eşlemesi: `BIDI_ENDPOINT_TO_SSE_METHOD_MAP[AgentService.run] = AgentService.runSSE`

Yani **giriş** `aiserver.v1.BidiService/BidiAppend`, **çıkış**
`agent.v1.AgentService/RunSSE`. Nexusor tam olarak bu ikisini yönlendiriyor.

> Daha önce dolaşıma giren "`RunSSE` ölü kod, her tur `Run`'a kaçıyor" iddiası
> **yanlıştır** ve düzeltilmiştir. `Run` yalnızca tanımdır; çalışma zamanında
> `RunSSE`'e eşlenir.

### 2.3 Açılan 7 bayrağın hepsi gerçekten okunuyor

`nal_agent_retries`, `explicit_subagent_models`, `subagent_support_interrupt`,
`opt_devs_into_experimental_model_toggle`, `meta_mcp_tool`, `mcp_input_schema_json`,
`glass_custom_modes` — yedisinin de `checkFeatureGate` / `L_` çağrı noktası var.

---

## 3. Bulgu 1 — Ölü araç kimlikleri (ÖNCELİK: KRITIK)

Cursor 3.24.12 kurulumunun **hiçbir dosyasında** geçmiyor:

| Nexusor tools.json | Cursor'da |
|---|---|
| `SembleSearch` | 0 geçiş |
| `SembleFindRelated` | 0 geçiş |

Cursor'un gerçek karşılığı `SEMANTIC_SEARCH` → model adı **`SemanticSearch`**
(`dsv3-1018` modelinde `codebase_search`).

### Uygulama notu — silinmedi, yeniden adlandırıldı

Bu iki aracın **çalışan uygulaması var**:

- `server/src/search/search_provider.rs` → `execute_semble()`
- `search.rs` → `SearchEngine::search()` / `find_related()` (semble-core)

Sonuç: uygulama silinmedi, **tel üzerindeki kimlik** düzeltildi.

- `SembleSearch` → `SemanticSearch` olarak yeniden adlandırıldı.
  Arka uç (semble-core) olduğu gibi kaldı.
- `SembleFindRelated` → Cursor'da karşılığı yok. Tel yüzeyinden **kaldırıldı**.
  Geri alınabilmesi için bu kayıtta bırakıldı.

### Ek hata: yanlış kart çizimi

`cursor/tools/codec/render.rs` → `tool_placeholder()` içinde `semblesearch` ve
`semblefindrelated` → `Tool::McpToolCall` ile çiziliyordu. Yani semantik arama
Cursor arayüzünde **generic MCP kartı** olarak görünüyordu.

Protokolde ayrı bir `SemSearchToolCall sem_search_tool_call = 16;` varyantı var ve
Nexusor'un kendi `protocols/cursor/agent_v1.proto` dosyasında zaten tanımlı.
Düzeltildi: artık `SemSearchToolCall` olarak çiziliyor.

---

## 4. Bulgu 2 — Eksik araçlar (ÖNCELİK: YÜKSEK)

Cursor 3.24.12'de kayıtlı 52 araç, denetimden önce Nexusor'da 24. Durum:

### 4.1 Tamamlandı

| Araç | Cursor kanıtı | Protokol varyantı | Nasıl çalışıyor |
|---|---|---|---|
| `SemanticSearch` | `hr("SEMANTIC_SEARCH")` | `SemSearchToolCall = 16` | Yeniden adlandırıldı, artık doğru kartı çiziyor |
| `LS` | `hr("LS")`, modele `LS`/`list_dir` | `LsToolCall = 13` | `Message::LsArgs` → Cursor'un kendi dizin listeleme yürütücüsü |
| `ListMcpResources` | `$W` kümesinde | `ListMcpResourcesToolCall = 20` | `Message::ListMcpResourcesExecArgs` → Cursor yürütücüsü |
| `Await` | `hr("AWAIT")` | `AwaitToolCall = 42` | **Nexusor yerel çalıştırır** (aşağıya bak) |
| `SendToUser` | `hr("SEND_TO_USER")` | `SendToUserToolCall = 58` | **Nexusor yerel** — kart çizimi teslimat |
| `SendFinalSummary` | `hr("SEND_FINAL_SUMMARY")` | `SendFinalSummaryToolCall = 49` | **Nexusor yerel** |
| `SendMessage` | `hr("SEND_MESSAGE")` | `SendMessageToolCall = 55` | **Nexusor yerel** — kablolandı, canlı denemede çağrılmadı |

Üç mesaj aracının hiçbiri `ExecServerMessage`'da yok; Cursor üçünü de kendi
agent-host'unda `executeToolCall` ile çalıştırıp anında yanıt veriyor.
Nexusor aynı şekilde yerel uygular ve yanıt metinlerini Cursor'unkilerle
birebir aynı tutar (`"ok"`, `"Message delivered to the user."`,
`"Final summary recorded."`).

Araç sayısı 24 → **29**.

### 4.2 Eklenmedi — gerekçesi

| Araç | Gerekçe |
|---|---|
| `ApplyPatch` | **Yol yok.** Cursor'un `ExecServerMessage`'ında `ApplyPatch` karşılığı **yok**. Agent-host patch'i kendi içinde parse edip `EditToolCall`'e çeviriyor (`hr("APPLY_PATCH")` → `executeToolCall({case:"editToolCall"})`). Nexusor bunu Cursor'a gönderemez; Rust'ta patch dili ayrıştırıcısı yazmak gerekir. Sessizce yanlış uygulanırsa bozuk dosya yazma riski var. |
| `GenerateVideo` | Protokolde `GenerateVideo` varyantı yok; ayrı yürütücü ve sağlayıcı gerektiriyor |
| `SendToUser`, `SendMessage`, `SendFinalSummary` | Varyantlar mevcut (`= 58`, `= 55`, `= 49`) ama çalışma zamanı ayrıntısı ayrıca doğrulanmadan eklenmedi |
| `CreateGoal`, `UpdateGoal`, `CreateTask`, `SetActiveBranch`, `RecordScreen`, `Reflect`, `SearchConversations`, `ConnectScm`, `CodeLineage`, `XSearch` | Varyantlar mevcut; her biri için yürütme yolu ayrı ayrı doğrulanmalı |

### 4.3 `Await` için özel durum

Cursor `Await`'ı kendi agent-host'unda çalıştırır; `ExecServerMessage`'da yalnız
`SubagentAwaitArgs` (alt ajan yoklama) vardır. Eklenen sürüm:

- **Uyku yolu çalışır** — `block_until_ms` kadar bekler, `AwaitToolCall` +
  `AwaitTaskComplete` döner. 300 saniye ile sınırlı.
- **Yoklama yolu çalışmaz** — `task_id`/`shell_id` verilirse modele açık bir hata
  döner. Modelin kontrol edilmemiş bir işi yokladığını sanması engellendi.

### 4.4 Kapsam dışı — doğrulandı, eklenmedi

| Araç | Gerekçe |
|---|---|
| `PI_READ`, `PI_BASH`, `PI_EDIT`, `PI_WRITE`, `PI_GREP`, `PI_FIND`, `PI_LS` | pi uyumlu alternatif ajan koşumu; ana araç setiyle birbirini dışlar |
| `MINI_SWE_AGENT_BASH` | mini-SWE-agent koşumu |
| `MOCK_READ_LINTS` | test ikizi |
| `REPLACE_ENV`, `SETUP_VM_ENVIRONMENT` | bulut devcontainer; protokolde `SetupVmEnvironmentToolCall = 33` var |
| `RECORD_CI_INVESTIGATION_FINDINGS` | bulut CI; protokolde `= 53` var |
| `browser_*` (16 araç), `computer_*` (19 araç) | MCP ile dinamik geliyor; statik katalog girdisi değildir |

---

## 5. Bulgu 3 — Telemetri sızıntısı (ÖNCELİK: YÜKSEK / GÜVENLİK) — **KAPANDI**

`local_app/proxy.rs` → `local_stub_status()` prefix ile şunları yutuyordu:
`AnalyticsService/*`, `MetricsService/*`, `CiMetricsService/*`,
`ClientLoggerService/*`, `PerformanceEventService/*`.

**Yutulmayan ve istemciden Cursor'a giden uç noktalar:**

| RPC | İçerik |
|---|---|
| `aiserver.v1.ProfilingService/SubmitProfile` | CPU profilleri |
| `aiserver.v1.WebProfilingService/SubmitInteractionWindow` | etkileşim pencereleri |
| `aiserver.v1.TraceService/SubmitSpans` | izleme span'leri |
| `aiserver.v1.ToolCallEventService/SubmitToolCallEvents` | araç çağrısı olayları |
| `aiserver.v1.ChatRequestEventService/SubmitChatRequestEvents` | istem olayları |
| `aiserver.v1.ChatRequestEventV2Service/SubmitChatRequestEventsV2` | istem olayları v2 |

Sonuç: "sıfır veri sızıntısı" iddiası bu 6 uç noktada **tutmuyordu**.

### Canlı kanıt

Sızıntı önce çalışan sunucu üzerinden ölçüldü (proxy `127.0.0.1:7745`,
Cursor'un yaptığı gibi POST ile):

| Uç nokta | Önce | Sonra |
|---|---|---|
| `ProfilingService/SubmitProfile` | **401 → upstream** | 200 / 0 byte |
| `TraceService/SubmitSpans` | **401 → upstream** | 200 / 0 byte |
| `ToolCallEventService/SubmitToolCallEvents` | **401 → upstream** | 200 / 0 byte |
| `ChatRequestEventService/SubmitChatRequestEvents` | **404 → upstream** | 200 / 0 byte |

`401`/`404` upstream'in reddidir; `200 / 0 byte` Nexusor'un yuttuğudur.

### Regresyon kontrolü

| Uç nokta | Sonuç | Anlamı |
|---|---|---|
| `AnalyticsService/BootstrapStatsig` | 200 / **1.617.699 byte** | Özellik keşfi bozulmadı |
| `NetworkService/IsConnected` | 200 / 5 byte | çalışıyor |
| `AiService/ServerTime` | 200 / 18 byte | çalışıyor |

Yakalama testleri eklendi: `every_event_upload_endpoint_is_swallowed_locally`
(11 yol) ve `statsig_bootstrap_is_not_swallowed`.

---

## 6. Bulgu 4 — RPC kapsama asimetrisi — **BULGU GEÇERSİZ**

Denetimde şu yazıyordu:

> `ListLocalSubscriptionTools` **yutulmuş**, `CallLocalSubscriptionTool` **yok** →
> araçlar listeleniyor, çağrıldığında upstream'e düşüyor.

**Bu hatalıydı.** İki nedenle:

1. **Stub listesi `local_account` kapısının arkasında.** `local_stub_status()`
   içinde `if !local_account { return None; }` vardır. Yani liste yalnız
   Nexusor'un *enjekte ettiği* yerel hesapta uygulanır.
2. **Kullanıcı gerçek bir Cursor hesabıyla çalışıyor.**
   `%APPDATA%\Cursor\User\globalStorage\state.vscdb` içindeki
   `cursorAuth/accessToken` gerçek bir Google OAuth token'ı
   (`cursorAuth/cachedEmail = genemiamk@gmail.com`, 424 bayt).
   `has_foreign_session()` gerçek oturum bulduğu için Nexusor hesap verisine
   dokunmuyor → `local_account = false`.

### Canlı ölçüm — gerçek token ile

| RPC | Sonuç | Nerede yanıtlandı |
|---|---|---|
| `DashboardService/GetGlobalCommands` | 200 / **24.057 byte** | gerçek upstream |
| `AgentService/ListLocalSubscriptionTools` | 200 / 0 byte | gerçek upstream (abonelik yok) |
| `AgentService/CallLocalSubscriptionTool` | **400 / 322 byte** | gerçek upstream — boş gövdeye verilen geçerli red |
| `AiService/ServerTime` | 200 / 18 byte | yerel |
| `AnalyticsService/BootstrapStatsig` | 200 / 1.619.021 byte | gerçek upstream |

`CallLocalSubscriptionTool`'ın 400/322 dönmesi, isteğin **Cursor sunucusuna
ulaştığını ve gerçekten işlendiğini** gösterir. Asimetri yoktur; liste de çağrı
da simetriktir.

Yerel (enjekte) hesapta ise liste boş döner, model hiçbir abonelik aracı
öğrenmediği için çağrı hiç oluşmaz.

**Sonuç:** Bu adım için yapılacak kod değişikliği yoktur.

### Aynı ölçüm telemetri düzeltmesini de doğruladı

`ProfilingService/SubmitProfile` gerçek token ile **200 / 0 byte** döndü.
Düzeltme `local_account` kapısının **dışında**, koşulsuz blokta olduğu için
resmî oturumda da yutuyor.

### Kalan açık soru

`McpToolService` 0/5 · `AgentStoreService` 0/24 · `ChatService` 0/14 ·
`FileSyncService` 0/9 · `CppService` 0/2 · `FullSelfDrivingService` 1/24 —
bunlar hâlâ yerel değil. Ancak yukarıdaki ölçüm gösteriyor ki **bu, resmî
oturumda sorun değil**: eşleşmeyen yollar gerçek tokenla upstream'e gidiyor ve
Cursor kendi yanıtını veriyor. Asıl soru, bunların **yerel (enjekte) hesap**ta
da aynı şekilde sorunsuz çalışıp çalışmadığı — o ayrı bir denetim ister.

---

## 7. Bulgu 5 — Feature gate (ÖNCELİK: ORTA)

805 bayrak var; 203'ü doğrudan okunuyor; Nexusor 7'sini açıyor.

### Bulgu 5a — Gerçek hesapta bayraklar hiç uygulanmıyordu — **DÜZELTİLDİ**

Denetimin ilk sürümü "7 bayrak doğrulandı" diyordu. Bu **yalnız yerel (enjekte)
hesap yolu için** doğruydu. Ölçüm gerçek hesapta farkını gösterdi:

`account.rs` içinde `if has_foreign_session(...) { return Ok(()) }` erken dönüşü,
gerçek bir Cursor oturumu bulunduğunda **hiçbir anahtarı yazmıyordu** — bayrak
override'ları dahil. `analytics.rs` ise resmî oturumda yalnız
`nal_websocket_client`'ı zorla kapatıyor.

Sonuç: gerçek Cursor hesabıyla çalışan bir kullanıcıda beş bayrak
(`explicit_subagent_models`, `subagent_support_interrupt`,
`opt_devs_into_experimental_model_toggle`, `meta_mcp_tool`,
`mcp_input_schema_json`) **sessizce hiç açılmıyordu**.

`byok.rs` gerçek hesaba yazdığı için BYOK çalışıyordu; sorun yalnız bayraktaydı.

**Düzeltme:** `workbench.experiments.featureFlagOverrides` bir hesap anahtarı
değil, Cursor'un kendi yerel experiment tercihi kanalıdır. Gerçek oturumda
`cursorAuth/*` ve gizlilik anahtarlarına dokunulmaz, yalnız bu anahtar yazılır.

### Bulgu 5b — Geri alma kaydı eziliyordu — **DÜZELTİLDİ**

`record.applied = applied.clone()` kaydın tamamını değiştiriyordu. Yalnız
bayrak yazan bir çalışma, daha önce yazılmış bir hesap kaydını düşürür ve o
anahtarlar geri alınamaz hâde gelirdi. Artık birleştiriliyor.

Her iki düzeltme için regresyon testi eklendi:
`gate_overrides_reach_a_real_session_while_account_keys_do_not` ve
`a_gate_only_run_keeps_an_earlier_account_record`.

### Bulgu 5c — Ultra görünümü kasıtlıdır — **BULGU GEÇERSİZ**

Denetimde "kullanıcının hesabında eski enjeksiyon kalıntısı var" yazıyordu.
**Bu yanlıştır.** Kullanıcının beyanı: Ultra görünümü **bilinçlidir** — yerel
özellikleri kullanabilmek ve gereksiz "Upgrade to Pro" uyarılarından kurtulmak
için gösterilir.

Bu, `AGENTS.md` → Uyumluluk kararları ile aynıdır: *"Yerel Ultra görünümünü veya
özellik ayarını kendi modellerimizle kullanılabilen yetenekleri açıyorsa
faydasını korumayı hedefle. Sırf resmî Free yanıtıyla aynı olsun diye bu
yetenekleri kapatma."*

Gerçek durum (ölçüldü):

| Anahtar | Değer | Not |
|---|---|---|
| `cursorAuth/stripeMembershipType` | `ultra` | **kasıtlı** |
| `cursorAuth/stripeSubscriptionStatus` | `active` | **kasıtlı** |
| `cursorAuth/accessToken` / `refreshToken` | gerçek Google token | kullanıcının oturumu, korunuyor |
| `cursorAuth/cachedEmail` | `genemiamk@gmail.com` | kullanıcının, korunuyor |
| `cursorAuth/stripeMembershipAuthId` | `google-oauth2\|user_01K7...` | kullanıcının, korunuyor |

Cursor'un `GetPlanInfo` yanıtı gerçek planın `Free` olduğunu söylüyor. Kasıtlı
olan yalnızca **görünüm** katmanıdır; oturum ve yetkilendirme tamamen
kullanıcının gerçek hesabına aittir.

### Bulgu 5d — Dikkat: bu değerlerin bakımı şu anda yapılmıyor

5a düzeltmesi gerçek oturumda `cursorAuth/*` anahtarlarına **hiç dokunmuyor**.
Bu, Cursor'un oturum açarken yazdığı gerçek değerleri korumak için doğru;
ancak `stripeMembershipType` / `stripeSubscriptionStatus` bilinçli bir görünüm
ayarı olduğundan **korunması da gerekir.** Bugün değerler duruyor çünkü daha
önce yazılmış; ancak Cursor oturum açarken kendi değerini yeniden yazarsa geri
gelmez.

### Derleme zamanı kapalı — hiçbir müdahale açamaz

```
localMode:!1  cursorPredictionOptions:!1
enableEmbeddingsModelToggle:!1  enableCPPControlTokenToggle:!1
```

`glass_local_ollama` bayrağı okunuyor ancak `C9p(t,e,n=kl.localMode)` ile
varsayılanı `localMode` olduğu için bu yapıda **etkisiz**.

### Yerel/BYOK ile ilgili, okunuyor ama kapalı

`agent_context_injection` · `agent_conversation_search_tool` ·
`agent_goal_continuation` · `agent_video_signed_url_uploads` ·
`auto_background_foreground_tools_on_followup` · `custom_agents_enabled` ·
`full_self_driving` · `glass_side_chats` · `glass_custom_theme_support` ·
`instant_grep_indexing` · `local_agent_transcript_search_in_glass` ·
`mcp_settings_overhaul` · `rules_discovery_respect_cursorignore` ·
`skills_cli_install` · `publish_user_skills` · `diff_tab_viewport_virtualization`

> Her biri için ayrı ayrı kazanç/kayıp analizi yapılmadan **açılmamalıdır**.
> `AGENTS.md` → Tercih ilkesi gereği önce birlikte çalışma araştırılır.

---

## 8. Doğrulama durumu

### Bu oturumda yapılan

| Kontrol | Sonuç |
|---|---|
| `cargo check --package cursor-server` | geçti |
| `cargo clippy --package cursor-server --all-targets -D warnings` | geçti |
| `cargo fmt --package cursor-server -- --check` | geçti |
| `cargo test --package cursor-server --lib` | **346 geçti, 0 düştü, 3 atlandı** |
| `tools.json` + 7 mod manifesti JSON ayrıştırma | geçti |
| **Canlı: sunucu yeniden başlatıldı, proxy geri geldi** | geçti |
| **Canlı: yerel RPC yönlendirmesi** | geçti |
| **Canlı: telemetri sızıntısı ölçüldü ve kapatıldı** | geçti |

### Hâlâ doğrulanmamış

- **Entegrasyon testleri (`server/tests/`) çalışmadı** — çalışan
  `cursor-server.exe` `target\debug\cursor-server.exe` dosyasını kilitliyor
  (Windows os error 5).
- **`cargo clippy --workspace` zaten başarısızdı** —
  `apps/desktop/src-tauri/src/desktop.rs` içinde `needless_return` hatası var ve
  bu hata HEAD'de bulunuyor, bu çalışmanın üretmediği bir sorundur.

## 9. Araçların canlı doğrulaması

12 Ekim 07:41'de gerçek bir Cursor oturumunda, `~/.nexusor/nexusor.db`
`tool_round_calls` tablosundan okunarak doğrulandı. Modelin anlatımına değil,
kayıtlara bakıldı.

| Araç | Sonuç | Kayıt |
|---|---|---|
| `LS` | ✅ çalıştı | `The directory is empty.` |
| `ListMcpResources` | ✅ çalıştı | `No MCP resources are available.` |
| `Await` | ✅ çalıştı | `Waited 7006 ms.` (6 sn'lik komut için doğru süre) |
| `Shell` (arka plan) | ✅ çalıştı | `shell running in background shell_id=920644` |
| `SemanticSearch` | ✅ çalıştı | `12.10 07:45` gerçek kod tabanında `handlers.rs` satırını buldu, `hata=0` |

`SemanticSearch` ikinci denemede (12.10 07:45) `C:\Users\StRonK\Desktop\Cursor`
üzerinde gerçek sonuç döndürdü: `query: "healthz endpoint route handler"` →
`Router::new() ... .route("/__byok-api__/healthz", get(health))`, `hata=0`.
İlk deneme boş klasörde çalıştığı için yeterli kanıt vermemişti; yeniden
adlandırma işlevi bozmadı.

### Kullanıcı raporundaki "arka plan görevi başlangıçta hata verdi" — çürütüldü

Model, arka plan görevinin hata verdiğini bildirdi. Kayıtlar bunu göstermiyor:

- Arka plan kabuğu **başarıyla** başlamış (`07:41:27`, `shell_id=920644`).
- `Await` **7006 ms beklemiş**, sonra `Read` ile çıktı dosyası okunmuş.
- Hata **ikinci, doğrulama amaçlı komutta** oluşmuş (`07:41:52`):
  `Windows sandbox helper only provides network proxy, not filesystem isolation`

Bu mesaj Cursor'un Windows'ta kendi sandbox'ının dosya sistemi izolasyonu
sağlamadığını söyleyen kendi mesajıdır. Veritabanında **10 Ekim'den beri 27 kez**
geçiyor; bu çalışmanın ürettiği bir sorun değildir. Arka plan + `Await` akışı
çalışıyor.

### Not: eski tel adı

`tool_round_calls` içinde 4 Ekim tarihli bir `SembleSearch` kaydı var.
`SemanticSearch`'e geçişten sonra bu ad dispatch ve kontrol noktasında
**uyumluluk takma adı olarak bırakıldı**, böylece eski checkpoint'lerin
tekrar oynatılması bozulmuyor.

### Dikkat: kullanıcının çalışma ağacına dokunuldu mu

`cargo fmt --all` çalıştırıldığında `apps/desktop/src-tauri/src/desktop.rs`
kullanıcının commitlenmemiş dosyası yeniden biçimlendirildi. Bu dosya `git checkout`
ile HEAD durumuna döndürüldü ve artık değişiklik listesinde yok.
Bu oturumun kapsamı dışıydı; bir daha yapılmayacak biçimde düzeltildi.

## 10. `SearchConversations` — canli testte bulunan dört gercek hata

Arac ilk kuruldugunde calistigi dogrulandi (5 cagri, hepsi `hata=0`), ancak
**ciktisi bozuktu**. Dört ayri hata bulundu; ilk ucu kendi yazdigim testler,
dorduncu canli veriye bakan bir denetimle yakalandi.

### 10.1 Snippet ham JSON donuyordu

Kayit:

```
- [495fe91c-…] 495fe91c — …"kind":"assistant","text":"","thinking":"",
  "tool_calls":[{"arguments":{ ...
```

`content` duz string degil, `{"kind":"parts","parts":[{"text":…}]}` seklinde.
Ilk cikarim duz string bekledigi icin `payload.to_string()`'e dusuyordu.

Duzeltme: `collect_text` yalnizca icerik tasiyan anahtarlari geziyor
(`text`, `parts`). Yapinin kendisini tarif eden uyeler (`kind`, `type`)
icerik sanilmaz; aksi halde sohbetin basligi `parts` olarak cikiyordu.

### 10.2 Turkce karakter uyumsuzlugu — eslesme vardi, bulunamadi

Kayit: `{"query":"guvenlik inceleme"}` → `No conversations matched.`

Depoda metin `guvenlik` yaziliydi; model diyakritiksiz yaziyordu. Eslesme
gercekten vardi. Duzeltme: `fold()` ile Turkce katlama
(`ç→c ğ→g ı→i ö→o ş→s ü→u ç→c`). Artik `guvenlik` → `güvenlik` buluyor.

### 10.3 Baslik sarmalayici adi oluyordu

Kayit: `- [495fe91c-…] <git_status> — …` ve `- [f3120b9b-…] (bos)`

Uctan uca inceleme (olcerek, varsayarak degil):

| Gozlem | Sonuc |
|---|---|
| Son 300 `user` mesajinda `<user_query>` | 175'te var |
| 12 konusmanin **ilk** `user` mesajinda `<user_query>` | **0/12** |
| Ilk `user` mesaji | 22 584–39 625 karakter, **tamami enjeksiyon** |

Yani her konusma tamamen sarmalayici bir turle aciliyor; gercek soru
`user#2`'de ve `<user_query>` icinde. Ilk mesajdan baslik cikarmak
`<user_info>` veya `<git_status>` uretiyor — kullaniciya tanidiktir degil.

Duzeltme:
- Baslik, konusma boyunca **en eski `<user_query>`** icinden alinir.
- `<user_query>` yoksa sarmalayicilar soyulur; kalan metin `Title::Other`.
- Ikisi de yoksa `Title::None`; son care `(untitled conversation 495fe91c)`.
- Baslik, yazilan sirayla degil **zaman damgasina gore en eski** olan secilir;
  satirlar yeniden eskiye dogru geldigi icin "ilk gordugunu tut" yanlisti.

Bilinen sarmalayicilar (`system_reminder`, `user_info`, `user_rule`, `rules`,
`agent_transcripts`, `available_skills`, `agent_skills`, `subagents`,
`mcp_meta_tools`, `mcp_meta_tool_servers`, `attached_files`,
`environment_details`) liste ile soyulur. Sozdege kalirsa ilk satir `<` ile
baslayan bir sarmalayici adi **baslik sayilmaz**.

### 10.4 Model conversation id uyduruyordu

Kullanici, sonucta gecen `f77ea055, 34545754, 91d1bd6d` kimliklerine tiklayinca
"chat not found" gordu. Bu kimlikler veritabaninda **yoktu**; model onceki
turlardan hatirlayip yeniden yaziyordu. Sebep: id koseli parantez icinde
veriliyordu, model ayrı satırda tekrar görmüyordu.

Duzeltme: kimlik artik kendi etiketli satirinda veriliyor ve sonucu birden
fazla eslesme icin `---` ile ayriliyor:

```
conversation_id: 495fe91c-d7e0-4898-839b-70f9d1ae0dd4
title: Create a long-running goal: read every file in this project and …
```

### Dogrulama

- 10 adet `conversation_search` testi, hepsi geciyor; tam paket **358/358**,
  `clippy -D warnings` temiz.
- **Canli veritabani denetimi:** son 16 konusmanin basliklari tek tek uretildi,
  **0/16 bozuk**. Ornekler:

| Konusma | Baslik |
|---|---|
| `495fe91c` | `Create a long-running goal: read every file in this project and …` |
| `ab928a58` | `List the files in this workspace using the LS tool.` |
| `518f8fa8` | `selam` |
| `83f97f85` | `Merhaba, bir test yapıyoruz. Bana geçerli dizindeki bir dosyayı …` |
| `35e1dd6d` | `Reply exactly NEXUSOR_CLEAN_BYOK_OK. Do not use tools.` |

### Not: arac yazarken sunucu birakma

Bu oturumda sunucu ~15 kez yeniden baslatildi; araya yeterli bekleme
konulmadigi icin proxy portu `TIME_WAIT`te kalip yeni surece rastgele port
(`4404`) seciliyordu. Ayni anda kullanici normal Nexusor'u acinca iki surec
ayni portu kovaladi; `settings.json` her seferinde yeniden yazildi.

Iki ayri hata sonucu ortaya cikti:

1. Kullanicinin gozunde sunucu "acilip kapaniyor" gibi gorundu.
2. Sunucu bir kez durdurulup `settings.json` temizlenmediginde Cursor olu bir
   proxy adresine yonlendi ve **modellere baglanamadi**.

Duzeltilen davranis: sunucu yalnizca gercekten gerekince baslatiliyor ve is
bitince kapatiliyor; kullaniciya acikca soyleniyor. Nexusor masaustu
uygulamasi ile gelistirme sunucusu ayni anda calistirilmiyor.
### 10.5 Bicim tahmin edilmedi — Cursor'un kaynagindan alindi

Kullanici sordu: *"formatini Cursor dosyalarindan niye almiyorsun?"* Hakliydi.
Bastan sona yazilmis iki bicim vardi (`conversation_id:` etiketli satir, ve
`Found N conversations` / `1. [local] ...`). Ikincisi daha dogruymus gibi
gorunuyordu ama o da tahmindi.

`cursor-agent-host/dist/main.js` icinde aracun kendi bicimlendiricisi bulundu
(`nQ`, indis `2202043`). Kaynak metni:

```js
function nQ(e){
  const t = 0===e.hits.length ? ["No matching conversations found."]
    : [`Found ${e.hits.length} conversation${1===e.hits.length?"":"s"}:`,
       ...e.hits.map((e,t)=>{
         const r = e.updatedAtMs===BigInt(0) ? "unknown"
                    : new Date(Number(e.updatedAtMs)).toISOString(),
               n = [`${t+1}. [${rQ[e.source]??"unknown"}] "${e.title}" (id: ${e.conversationId}, updated: ${r})`];
         return void 0!==e.snippet && e.snippet.length>0
                  && n.push(`   snippet: ${e.snippet}`), n.join("\n");
       }(e,t))];
  return e.truncated && t.push("Additional matches were omitted because the result limit was reached."),
         e.rebuilding && t.push("The conversation search index is rebuilding; results may be incomplete."),
         e.partial && t.push("Search stopped early; partial results were returned."),
         t.join("\n")
}
```

Ayni dosyadan alinan kaynak etiketleri:

```js
rQ = { UNSPECIFIED: "unknown", LOCAL: "local", CLOUD_CACHE: "cloud-cache" }
```

**Yapilanlar:**

| Konu | Once | Sonra |
|---|---|---|
| Bos sonuc | kendi metnim | `No matching conversations found.` |
| Satir bicimi | `conversation_id:` etiketli blok | `1. [local] "…" (id: …, updated: …)` |
| Snippet | ayri satirsiz gomulu | `   snippet: …` |
| `source` | `0` → `unknown` olarak okunurdu | `1` → `local` |
| `limit` ust sinir | 50 | 100 (Cursor: 1–100) |
| Kesilen sonuc | sessizce kayboluyordu | `Additional matches were omitted because the result limit was reached.` |
| `query`/`limit` aciklamalari | kendi yazimim | Cursor'un `describe()` metinleri birebir |

`truncated` alani protokolda zaten vardi ve hep `false` gonderiliyordu; artik
gercekten esik asildiginda `true` gidiyor.

### 10.6 Snippet de ayni hastaliktan kurtuldu

Baslik icin yazilan sarmalayici soyme `snippet` icin uygulanmamisti; modelin
onune `</timestamp>` ve `<user_query>` gidiyordu. Ikisi de **tek bir**
`readable()` fonksiyonundan besleniyor.

Bunun bir yan etkisi var ve bilincli: arama artik sarmalayici metin **uzerinde**
degil, okunabilir metin uzerinde calisiyor. Cursor'un kendi `query` aciklamasi
da ayni siniri koyuyor: *"search for in conversation titles and **visible**
user/assistant message text"*. Yani bu bir kisit degil, Cursor'un semasi.

### 10.7 Cursor'un bu arac icin kendi arayuzu YOK

`workbench.desktop.main.js` icinde:

```js
case "searchConversationsToolCall": { p = nt.UNSPECIFIED; break; }
case "searchConversationsToolCall": { i.tool = nt.UNSPECIFIED; break; }
```

Yani Cursor bu arac callusunu **`UNSPECIFIED` olarak isliyor**; ozel bir kart
render'i yok. Kullanicinin gonderdigi ekrandaki guzel kart, **modelin yazdigi**
markdown link (`[Security Review and Long-Running Goal](495fe91c-…)`) ve
Cursor'un genel konusma-link bileseni. Baslik o linkin etiketi — yani modelin
yazdigi metin, benim `title` alanim degil.

Sonuc: **bu arac icin "Cursor'a yaptir" secenegi yok.** Metin uretimi bizde,
render Cursor'da. Dogru yaklasim Cursor'un bekledigi metni uretmek — 10.5'te
yapilan tam olarak bu.

### Dogrulama (10.5–10.7 sonrasi)

- 19 adet `conversation_search` testi; tam paket **367/367**, `clippy -D warnings` temiz.
- Canli veritabani denetimi, sorgu `guvenlik inceleme`:

```
Found 1 conversation:
1. [local] "Create a long-running goal: read every file in this project and write a"
   (id: 495fe91c-d7e0-4898-839b-70f9d1ae0dd4, updated: 2026-10-12T07:12:50.646Z)
   snippet: Geçmiş konuşmalarınızda **"guvenlik inceleme"** aramasıyla doğrudan
   eşleşen konuşma: …
```

- Sarmalayici sizinti denetimi: etiket iceren snippet **0**, bozuk baslik **0**.
- Testlerden biri (`the_layout_matches_cursors_own_renderer`) tam olarak
  beklenen metni sabitler; Cursor'dan kopulursa kirmizi olur.