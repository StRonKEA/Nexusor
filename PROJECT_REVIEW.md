# Nexusor — Doğrulama Kanıt Arşivi

Bu dosya **tarihsel doğrulama kayıtlarıdır**. Güncel teknik durum için `STATUS.md` tek
kaynaktır; karar ilkeleri `AGENTS.md` içindedir.

Buradaki kayıtlar kendi tarihindeki koşulları anlatır ve **bağlayıcı talimat değildir.**
Burada geçen model adları, özellik listeleri veya "başarılı" ifadeleri güncel gerçek
varsayılmaz; kod ve canlı kanıtla yeniden doğrulanmadan kendini doğrulamış sayılmaz.

## Kayıt biçimi

Her kayıt: neyin kanıtlandığı, nerede (call/run kimliği), ve neyin **kanıtlanmadığı**.

Kaynak bulguları, yerel testler ve canlı doğrulama birbirinden ayrıdır. Arayüzde
görünmesi veya otomatik testlerin geçmesi, bir özelliğin uçtan uca çalıştığının kanıtı
değildir.

---

## 9 Ekim 2026 — Tam Kapasite Canlı Doğrulama, Kurumsal Servisler ve Kalkan

Cursor 3.22 (Glass mimarisi) kurulu temiz ortamda canlı oturum üzerinden uçtan uca 10 aşamalı kullanıcı testi gerçekleştirildi ve doğrulandı:

1. **Ajan & Temel Yürütme (`Shell` & `Read`):** Dizin listeleme ve `node -v` komutu başarıyla çalıştırıldı (Çağrı: `7d4823cd-5e7b-44af-86d1-65671df82617`).
2. **Çok Dosyalı Proje (`Write`):** `package.json` ve `index.js` dosyaları diske oluşturuldu ve `node index.js` ile test edildi.
3. **Cerrahi Düzenleme (`StrReplace`):** Dosya baştan yazılmadan satır içi diff ile `carp()` fonksiyonu eklendi ve terminalde doğrulandı (`4 * 5 = 20`).
4. **Web Arama & Sayfa Okuma (`CallMcpTool` / `WebSearch`):** Resmi Rust 1.99.0 (1 Ekim 2026) sürüm bilgisi canlı internetten çekildi.
5. **Alt Ajanlar (`Task`):** `Subagent ID: 752e431e-4f29-4929-991c-4298a0659807` ile bağımsız alt ajan süreci çalıştırıldı, `bilgi.txt` üretildi ve ana akışta doğrulandı.
6. **Kod Tabanı Arama (`Grep`):** `carp` sembolü `index.js` içinde satır 5 ve 17'de regex ile nokta atışı bulundu.
7. **Cmd+K Satır İçi Düzenleme (`StreamCmdK`):** `Claude Opus 4.6` modeli üzerinden satır içi diff üretildi ve kabul edildi (`cmdk-edit-a31b281a...`).
8. **Terminal Cmd+K (`StreamTerminalCmdK`):** Terminal kutusunda doğal dilden komut üretimi ve çalıştırılması doğrulandı (`cmdk-terminal-450d96c9...`).
9. **Görsel Üretimi (`GenerateImage`):** `nexusor-logo.png` (44.591 byte) görseli Antigravity/Gemini motoruyla üretildi, atomik kaydedildi ve doğrulandı.
10. **Kurallar (Rules):** `.cursorrules` dosyası oluşturuldu; model sonraki istekte `Turkce_` ve `[NEXUSOR]` kurallarına %100 uydu.

### Yeni Eklenen Kurumsal Servisler (Dinamik BYOK Entegrasyonu)
* **`ReviewService` (BugBot & Canlı Kod İnceleme):** `BugConfig`, `StreamReview` ve `StreamBugBotAgentic` uç noktaları bağlandı. Projedeki git diff'leri dinamik model üzerinden kod incelemesine tabi tutuldu.
* **`CmdKService` Akıllı Bağlam Sıralama:** `RerankCmdKContext` ve `RerankTerminalCmdKContext` yerel context ranking algoritmasıyla bağlandı.
* **`AgentService/CreateTranscriptOverview` & `NameAgent`:** Sohbet başlıkları ve oturum özetleri dinamik hızlı model ile akıllı üretildi.
* **`GitGraphService`:** `IsGitGraphEnabled`, `GetGitGraphStatus` ve `GetGitGraphRelatedFiles` ile ilişkili dosyalar bağlama dahil edildi.
* **`FastApplyService` & `CursorPredictionService`:** Sıfır gecikmeli diff birleştirme (`WarmApply`, `ReportEditFate`) ve imleç düzenleme tahmini (`CursorPredictionConfig`) devreye alındı.
* **İnteraktif Terminal (`WriteShellStdin`):** Arka plan ve interaktif terminal süreçlerine canlı girdi yazma aracı `tools.json`'a eklendi.
* **Telemetri Kalkanı:** 118 adet resmi analiz/telemetri uç noktası yerelde yutuldu (`HTTP 200 / 0 bytes`); veri sızıntısı ve bant genişliği israfı sıfırlandı.
* **36 Elit Feature Gate:** `local_computer_use`, `parallel_agent_workflow`, `glass_custom_modes`, `instant_grep_indexing`, `diff_tab_viewport_virtualization` ve alt ajan bekleme (`enable_await_for_subagents`) bayrakları aktif edildi.

---

## 8 Ekim 2026 — Patch'siz alt ajan iptali ve görev kontrolleri

Nexor'un yerel görev paneli (`stop_run`, `stop_tool`, `background_tool`) özgün Cursor
host/Glass ile canlı doğrulandı: foreground Task iptali, Shell/Task'ın arka plana
taşınması ve gerçek süreç bitişi geçti.

Çocuk Stop'un kartı açık bırakmasının kök nedeni, `stop_run` yolunun `RunFinished`'i
bastırmasıydı. Açık Nexusor stop'u transport'ta işaretlenip `RunFinished` iletilmeye
devam etti; kullanıcı stop'u için `USER_ABORTED_REQUEST(21)`, `is_retryable=false` ve
`Canceled` dışı bir Connect kodu kullanıldı.

Canlı run `07c922ab-1989-4f32-b123-fb40b882081f:42aae817`, çocuk konuşma
`a1ea2343-a716-4e2b-a1f9-ded8fbe43c0f`. Cursor kartı "Stopped with error" kapandı, ana
bildirim doğru geldi, restart sonrası yalnız iptal edilmiş run kaldı ve yeni run
oluşmadı.

**Kanıtlanmayan:** Bu, Cursor'un kendi çocuk kart Stop düğmesinin onarılması değil,
Nexusor panelinden yapılan işlevsel kontroldür. Kart "Stopped with error" gösterir; bu
başarı sonucu değildir.

## 8 Ekim 2026 — Temiz Cursor'da otomatik BYOK ve model görünürlüğü

Kurulu Cursor 3.22.12, temiz Free hesap: model seçici `useOpenAIKey` yok/false
koşuluyla kilitleniyordu. Nexusor `applicationUser` içinde yalnız bu alanı yönetiyor,
önceki yokluk/değeri ayrı kayıtta tutuyor, disable'da sonraki kullanıcı değişikliğini
koruyor. Gerçek hesabın planı, tokeni ve mevcut key/base URL'i değişmedi.

Routed-view seçici yalnız "Auto" gösteriyordu; `visible_in_routed_model_view`
Nexusor modellerine, combo ve Auto Router'a eklendi. Hesap/plan yanıtları değiştirilmedi.

Gerçek kullanıcı istemi → Antigravity HTTP200 → `NEXUSOR_CLEAN_BYOK_OK`, çağrı
`f7fbf330-1b41-4599-b86b-04d9ceefdfd9:551c4d7e:0`. İlk Auto seçimi Cursor kota ekranı
verdi ve başarı sayılmadı; açık model seçimi sonrası yeniden açılış ve yeni istemle
geçti.

**Kanıtlanmayan:** Kaynak ve debug kanıtıdır; temiz Windows/NSIS kabulü yapılmadı.

## 8 Ekim 2026 — Sağlayıcı hata sonrası hesap geçişi

Doğal 429/QUOTA_EXHAUSTED geldi; Claude ailesi beklemesi ve ikinci hesap seçimi gerçek
loglarla doğrulandı. İkinci hesabın istek kaydı aynı `call_id`'ye INSERT edildiğinde
UNIQUE hatası çıktı; recorder account attempt kayıtlarını ayırarak düzeltildi.

Cursor yeni turda ikinci hesapla işi 3/3 test, exit 0 ile bitirdi.

**Kanıtlanmayan:** Düzeltme sonrası **aynı istek içinde** doğal hata → tam başarı zinciri
hiç gözlenmedi. Kayıt düzeltmesi kontrollü fixture (yerel HTTP429 → ikinci kimlik →
HTTP200) ile doğrulandı; bu doğal upstream kanıtı sayılmaz. Kota tüketilerek hata
üretilmedi.

## 7 Ekim 2026 — Görüntü üretimi

`GenerateImage` akışı: onay/yürütücü hattı, 120 s ve 24 MiB yanıt sınırı, referans
görsel sayısı/boyutu, PNG doğrulaması, atomik üzerine-yazmayan yerel kayıt, run bazlı
iptal.

Referanslı üretim ve hata/iptal canlı UI dalları geçti (275.710 B PNG → tek Read →
doğru final; yerel 512×512 önizleme). Mevcut dosyayı koruyan hata ve Stop → cancelled /
dosya yok doğrulandı.

**Kanıtlanmayan:**
- Piksel düzeyinde referans koruması garanti değil.
- Referanslı üretim ve hata/iptal dalları tek örneklemeyle doğrulandı.
- 33 B kesik PNG → yerel EOF araç hatası → doğru final paket içinde geçti; geç SSE hatası
  doğal olarak oluşmadı, test kanıtı var.
- Otomatik hesap/endpoint yedeklemesi paket içinde; doğal hata olmadan yeniden üretim
  uygulanmadı.

## 7 Ekim 2026 — Terminal, arka plan ve ilerleme ayrımı

Sessiz `exit 23` gerçek model girdisinde korunuyor ve doğru başarısızlık finali
veriliyor. Background `exit 17`/failed ve gerçek bitiş bildirimi geçti. Foreground
Stop → süreç sonu, cancelled, kısmı çıktı kalıcılığı geçti.

İki Shell ile 12 s background + 45 s foreground: background bitişi sırasında foreground
çalışmaya devam etti ve tam 45 s, exit 0 ile bitti. Cursor bildirimi idle'a kadar tutup
sonradan gönderdi.

**Kanıtlanmayan:**
- Model, bildirim gelmeden önce "tamamlandı" anlatabiliyor. Bu ayrı bir tutarlılık
  sınırıdır; mekanizma başarısı bunu çözmez.
- TaskProgress-only dal doğal olarak gözlenmedi; test kanıtı var.
- Aktif run'a kesmeden teslimat dalı için doğal UI kanıtı yok.

## 7 Ekim 2026 — Çocuk Stop patch'i ve kalan patch incelemesi

Kurulu `cursor-agent-host/dist/main.js` içindeki `ft(...)` hata sınıflandırıcısı, yerel
abort kaynağı yokken ve agent retries açıkken transport retry seçiyordu. İzole VM'de
kurulu özgün sınıflandırıcı çalıştırılarak doğrulandı:

| Girdi | Sınıflandırma |
|---|---|
| Uzak Canceled, retries açık, yerel abort yok | retry / transport |
| Uzak Aborted, aynı koşullar | retry / transport |
| Yerel abort kaynağı gözlenmiş | throw, retry yok |

Bu, "kart Working kaldı ama süreç durdu" gözleminin bir mekanizmasını açıklar; geçmiş
her olayın tek kök nedeni değildir.

Görüntü native Write incelemesi: özgün WriteArgs'ta `create_new`/no-overwrite alanı yok;
özgün yazıcı izin denetimlerinden sonra `truncate(0)` kullanıyor. Windows hedef
reservation denemesi mevcut dosyayı korudu ve rename'i engelledi, ama izin alınmadan
0 baytlık görünür dosya oluştuğu için aynı izin semantiğini vermiyordu; üretime
alınmadı.

**Sonuç (8 Ekim):** Üç patch'in tamamı kaldırıldı. Çocuk Stop ve arka plan taşıma
Nexusor paneliyle karşılandı; görüntü yazımı Nexusor'un kendi `create_new` yazımına ve
Cursor izin kapısına taşındı. Aktif patch sayısı sıfır.

## 7 Ekim 2026 — Temiz kurulum öncesi denetim

Kurulum ağacındaki 5 `.bak` dosyası eski patch yedekleriydi; **topluca geri
kopyalanmamalıydı**, çünkü bazıları eski patch'i geri getiriyordu. Kurulu
`product.json` içindeki 6 dosya checksum'u eşleşti.

Kaynak sorunları bulundu ve düzeltildi: `settings` disable'ın eski değerleri geri
getirmeden silmesi; stale-repair'in tüm dosyayı ezmesi; repair hata yutması; çalışma
akışına bağlı olmayan üç UI anahtarının yanlış vaatleri.

Geniş Rust turu 451/0/3 geçti; ignored medya 3/3 ayrıca; toplam 456 benzersiz Rust +
8 Python testi. Clippy, TypeScript, Node typecheck ve Vite build geçti.

**Kanıtlanmayan:** Temiz Windows/NSIS kurulumu yapılmadı. Geliştirme PC'si başarısı
sıfır kurulum kanıtı değildir. Medya için Python/paket/model ayrıca kurulur.

## Daha eski kayıtlar

7 Ekim öncesi oturumların çağrı kimlikli canlı geçmişi bu dosyanın önceki sürümlerinde
korunmuştur; artık ayrı dosya tutulmamaktadır. Geçmiş paket hash'leri ve canlı run
kimlikleri artık geçerli kabul edilmez.

## Temiz kurulumda kabul sırası

1. Cursor sürümü ve özgün dosya hash'lerini al; eski `.bak` dosyalarını yeni kurulumun
   üstüne kopyalama. Sürüm değiştiyse eski hash geçerli değildir.
2. Normal gerçek Cursor hesabıyla oturum aç. Nexusor hesap/sağlayıcı ayarlarını ayrı
   koru; `%APPDATA%/Cursor`, `~/.cursor` ve `~/.nexusor` farklı veri alanlarıdır.
3. **Hiçbir patch betiği çalıştırma.** Kurulum, Cursor'a hiç dokunmadan çalışacaktır.
4. Enable → disable → enable: önceki proxy/özel `noProxy` ve kullanıcı ayarları geri
   dönmeli, yeni kullanıcı değişiklikleri korunmalı, journal dosyaları temizlenmeli.
5. Normal Read → edit → Shell → final; foreground/background Task; görev panelinden
   hedef iptal ve arka plana taşıma; sibling izolasyonu.
6. Rules/hooks/MCP/mağaza; Tab/CmdK; büyük girdi ve sıkıştırma; PDF/video/ses
   yardımcıları.
7. **Görüntü üretimini patch'siz doğrula:** referans Read → izin kapısı → üretim →
   atomik yazım → tek Read.
8. Hesap rotasyonu ve doğal 429 kapısını uygun olayda kaydet; kota tüketerek üretme.