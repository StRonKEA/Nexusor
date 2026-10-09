# Nexusor — İlk kurulum (Windows)

## Temel kodlama/ajan kullanımı

1. Nexusor NSIS kurulumunu tamamlayıp uygulamayı açın. WebView2 Runtime masaüstü arayüzü için gereklidir; eksikse kurulum sırasında indirme gerekebilir.
2. Cursor'u ayrıca kurun ve gerçek Cursor hesabınızla oturum açın. Nexusor Cursor kurulumunu veya oturumunu sağlamaz.
3. Nexusor'da kullanacağınız sağlayıcı hesabını/API anahtarını ekleyin; model kataloğunu yükleyip erişebildiğiniz bir modeli etkinleştirin. Önceki PC'deki hesaplar, tokenlar ve kayıtlı model listesi yeni kurulumda hazır gelmez.
4. Nexusor'un Cursor entegrasyonunu kurup etkinleştirin. Gerekirse uygulamanın gösterdiği yerel CA sertifikası kurulum adımını tamamlayın. Nexusor gerekli Cursor BYOK kullanım ayarını otomatik etkinleştirir; önceki değeri saklar ve kapatırken sonraki kullanıcı değişikliklerini koruyarak geri yükler. Ayar değişecekse açık Cursor yeniden başlatılır; kaydedilmemiş çalışmalarınızı önce kaydedin. Entegrasyon durumunda CA ve ayarların hazır olduğunu kontrol edin.
5. Cursor'u yeniden açın, çalışma ortamını **This PC / Local** seçin ve Nexusor üzerinden kullanacağınız modeli açıkça seçin. Cursor Auto seçimi ile Nexusor Auto Router aynı ayar değildir.
6. Küçük bir yerel dosyayı okuma ve basit terminal komutu ile başlayın. Terminal görevleri için kullanılan araçlar (ör. Git, Node.js veya projenin derleyicisi) PC'de ayrıca bulunmalıdır.

Nexusor release uygulamasını kullanmak için kaynak kod klasörü, Rust veya npm kurulumu gerekmez. Veritabanı/ayarlar kullanıcı profilindeki `.nexusor` dizininde ilk açılışta oluşturulur. Sağlayıcı ve Cursor çevrim içi hizmetleri internet ve ilgili hesapların erişimini gerektirir.

## Cursor kurulumuna dokunulmaz

Nexusor, **Cursor'un hiçbir program dosyasını değiştirmez.** Kurulum paketinde Cursor'a
patch uygulayan hiçbir betik bulunmaz ve Nexusor hiçbir aşamada Cursor dosyasına yazmaz.

Ne yazıyorsa yalnız kendi verisine yazar ve bunların hepsini geri alınabilir:

| Ne | Nerede | Kapatınca |
|---|---|---|
| Proxy/CA ayarları | `%APPDATA%\Cursor\User\settings.json` | Yazdığı her anahtarın önceki değeri geri yüklenir |
| Ayar geri alma kaydı | `settings.json.nexusor-managed.json` | Disable ile temizlenir |
| Yerel hesap/plan görünümü | `state.vscdb` + `state.vscdb.nexusor-managed.json` | Yazdığı 10 anahtarın tamamı geri yüklenir |

Gerçek bir Cursor oturumunuz varsa Nexusor hesap verisine dokunmaz; yalnız kendi
model yönlendirmesini devreye alır.

## İsteğe bağlı PDF/video/ses desteği

PDF metin çıkarımı temel uygulamadadır. Taranmış PDF sayfalarının görüntüsü, video kareleri ve yerel ses çözümleme ek Python ortamı gerektirir. Bu ortam ve konuşma modelleri NSIS içinde gömülü değildir.

Windows x64 Python 3 kurulu ve `python` komutu erişilebilir olmalıdır. İlk kurulum paketleri/modeli internetten indirir. Nexusor kurulum dizininde PowerShell açıp aşağıdaki komutları çalıştırın:

```powershell
# PDF sayfa görüntüsü ve video kareleri
& ".\scripts\setup-media.ps1"

# Ek olarak yerel ses çözümleme (base)
& ".\scripts\setup-media.ps1" -Speech

# Daha büyük isteğe bağlı ses modeli
& ".\scripts\setup-media.ps1" -Speech -SpeechModel small
```

PowerShell ilkesi betiği engelliyorsa yalnız bu çalıştırma için:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File ".\scripts\setup-media.ps1" -Speech
```

Kurulum kullanıcı profilindeki `.nexusor/media-runtime` ve `.nexusor/media-models` dizinlerini kullanır. Yönetici hesabıyla başka kullanıcıya kurmak yerine Nexusor'u kullanan Windows kullanıcısı olarak çalıştırın. Betik hata verirse medya desteği kurulmuş sayılmaz; temel kodlama araçları bu isteğe bağlı ortama bağlı değildir.

Ses analizi ilk 60 saniye ve 30 saniye çalışma süresiyle sınırlıdır; small yavaş PC'lerde süre aşımına uğrayabilir. Görüntü üretimi ayrıca erişilebilir, yapılandırılmış görüntü üretim modeli/hesabı gerektirir.

## Doğrulama sınırı

### Nexusor üzerinden yerel görev kontrolü

Nexusor **Cursor → Yerel görev kontrolleri → Yenile** bölümünde etkin çalışma ve
Shell/Task'ı seçin. **Arka plana taşı** mevcut işlemi taşır; **Durdur** hedef aracı,
**Bu çalışmayı durdur** seçili run'ı iptal eder. Konuşma/run kimliğini kontrol edin.
İstek alındı yanıtı tamamlanma bildirimi değildir; gerçek sonucu Cursor'da izleyin.
Kontrol Nexusor panelindedir; Cursor'un kendi çocuk kartı düğmesinin davranışı
değiştirilmez ve değiştirilmez de kalmalıdır.

Bağımsız background run iptali yapılandırılmış kullanıcı-iptal hatasıyla sonlandırılır.
Cursor kartında "Stopped with error" gösterimi olabilir; bu bir başarı sonucu değildir.
Shell hazırlık aşamasındaki taşıma isteği yalnız mevcut aktif turda saklanır ve araç
başladığında uygulanır; ikinci komut oluşturulmaz.

## Görüntü üretimi

GenerateImage hiçbir Cursor dosyasına dokunmadan çalışır.

- **Referans görseller** Cursor'un kendi Read yürütücüsünden okunur, yani Cursor'un izin
  ve onay akışı aynen geçerlidir.
- **Hedef dosya** üretim başlamadan önce Cursor'un Read yürütücüsünden geçirilir. Kullanıcı
  erişimi reddederse hiçbir üretim yapılmaz ve dosya yazılmaz.
- **Yazma** Nexusor tarafından `create_new` ile yapılır. Bu tek bir dosya sistemi işlemidir:
  mevcut dosyanın üzerine yazılmaz, yarış koşulu oluşmaz ve dosya varsa işlem açık bir
  hata mesajıyla durur.
- Mevcut dosyayı korumak için üretilen yeni bir isim seçin.

Kaybedilen tek şey, dosyanın Nexusor tarafından yazılması nedeniyle Cursor'un *Write*
aracının etkinlik kaydında görünmemesidir. Cursor'un Read/izin/onay semantiği korunur.

## Önceden patch'li bir kurulumunuz varsa

Daha önceki Nexusor sürümlerinden patch uygulanmış bir Cursor kurulumu kullanıyorsanız,
özgün dosyalara dönmek için arşiv betikleri sadece bu iş için tutulur. **Yeni kurulumda
çalıştırmayın.** Kurulum paketine dahil değildirler.

```powershell
pwsh -File scripts\legacy\fix-cursor-image-write.ps1 -Check
pwsh -File scripts\legacy\fix-cursor-image-write.ps1 -Restore
```

Ayrıntı ve kalan iki betik için `scripts/legacy/README.md`.

## Genel sınır

Geliştirme PC'sindeki çalışan exe testleri tamamen temiz Windows/NSIS kurulum testiyle
aynı değildir. Güncel doğrulama kayıtları `STATUS.md` ve proje raporlarındadır; bütün
sağlayıcılar ve bütün PC yapılandırmaları için sorunsuz çalışma garantisi verilmez.