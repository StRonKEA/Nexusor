# Nexusor — Kalıcı amaç ve çalışma ilkeleri

Bu dosya proje genelinde çalışan yapay zekâ ajanları için temel bağlamdır. Kullanıcının aynı amacı her oturumda yeniden açıklamasını gerektirmeden çalışmayı sürdür.

## Ana hedef

**Cursor'un bütün özelliklerini, Cursor'un hiçbir dosyasına müdahale etmeden, Nexusor üzerinden kullanıcının kendi modelleri ve sağlayıcı hesaplarıyla yönetilebilir hale getirmek.**

Amaç iki katmanlıdır:

1. **Yönetilebilirlik:** Model, sağlayıcı, hesap, kota, yönlendirme, araçlar, ajan döngüsü ve özellik görünürlüğü tek yerden — Nexusor'dan — yönetilir. Kullanıcı, özellik açmak için Cursor'un kendi arayüzüne veya ayarlarına elle gitmez.
2. **Değişmezlik:** Bu yönetilebilirlik, Cursor'un kurulu JS/exe/kurulum dosyalarına **hiçbir şekilde yazılmadan** sağlanır.

Kullanıcının "kesin zorunluluk varsa patch uygulanabilir" istisnası aşağıdaki Patch İlkesi'nde tanımlıdır. Bu istisna **son çaredir**; projedeki varsayılan değildir.

**Çalıştırma kapsamı:** Ajan görevleri, terminal, dosya düzenleme ve alt ajanlar yalnızca kullanıcının PC'sinde çalışacak. Cursor'un Cloud seçeneği kullanılmayacak; mümkünse yerel çalışmayı ve gereken çevrim içi hizmetleri bozmadan kapatılacak. Cloud agent, bulutta otomasyon, uzak repo oluşturma/yükleme ve bunlar için GitHub/GitLab veya takım bağlantısı kurma bu projenin hedefi değildir. Yerel arka plan/paralel ajanlar kapsam içindedir. Model sağlayıcılarına erişim, gerçek Cursor oturumu, web araması, MCP ve mağaza gibi gereken çevrim içi hizmetler bu karardan ayrı tutulur.

Hedef yalnızca MCP veya modele mesaj gönderme değildir. Ajan, planlama, alt ajanlar, paralel görevler, kod tabanı araması, indeksleme, bağlam toplama, web araması, tarayıcı, görüntü işleme, terminal, dosya düzenleme, Cmd+K, otomatik tamamlama, rules, skills, hooks, MCP, mağaza ve çevrim içi entegrasyonlar birlikte değerlendirilmelidir. Bu liste kapsamı sınırlamaz.

Gerçek Cursor çevrim içi hesabı, oturum ve çevrim içi hizmetleri kullanabilmek için bu mimarinin parçasıdır. Gerçek hesabı kullanmak, bütün model isteklerini Cursor'un modellerine göndermek anlamına gelmez.

## Hedeflenen deneyim ve özellik kapsamı

Kullanıcı Cursor içinde bir işi tarif ettiğinde; Cursor'un işi araştırması, planlaması, uygun araçları kullanması, kodu değiştirmesi ve sonucu doğrulaması hedeflenir. Nexusor bu deneyimin arkasında kullanıcının seçtiği modelleri ve hesapları çalıştırır. Amaç, model sağlayıcısı değişirken Cursor'un bütünleşik çalışma deneyiminden mümkün olan en fazla faydayı korumaktır.

- **Ajan, planlama, alt ajanlar ve paralel görevler:** Çok adımlı işleri yürütme, işi parçalara ayırma, uzman alt ajanları kullanma, görevlerin bağlamını ve sonuçlarını ana akışta birleştirme.
- **Kod tabanı araması, indeksleme ve bağlam toplama:** Dosyaları, sembolleri, ilgili kod parçalarını ve proje kurallarını bulma; modele doğru bağlamı taşıma; uzun konuşmalarda bağlam sürekliliğini koruma.
- **Web araması, tarayıcı kullanımı ve görüntü işleme:** Güncel bilgi ve dokümantasyon bulma, sayfaları okuma ve etkileşim kurma, ekran görüntülerini modele aktarma, yapılan işi tarayıcı üzerinden doğrulama.
- **Terminal, dosya düzenleme, Cmd+K ve otomatik tamamlama:** Komut çalıştırma, test ve derleme sonuçlarını değerlendirme, çok dosyalı ve satır içi düzenleme, değişiklikleri inceleme, Tab/tamamlama akışlarından yararlanma.
- **Rules, skills, hooks, MCP ve mağaza:** Proje talimatlarını uygulama, yeniden kullanılabilir yetenekleri ve araçları keşfetme, kurma, yetkilendirme ve gerçek görevlerde çalıştırma.
- **Çevrim içi entegrasyonlar:** Hesap ve takım hizmetleri, repository bağlantıları, paylaşlanan içerikler ve erişilebildiği ölçüde yararlanma. Bulut agent otomasyonu kapsam dışıdır.
- **Model ve hesap yönetimi:** Göreve uygun model seçimi, combo/Auto Router, kota ve hesap rotasyonu, hata sonrası alternatif sağlayıcıya geçiş; araç çağrıları ve konuşma devamlılığını bu seçimlerle uyumlu tutma.

Bu başlıklar hedef ve inceleme kapsamıdır; hepsinin bugün çalıştığına dair bir destek beyanı değildir. Cursor'a yeni yetenekler eklendikçe kapsamı yeniden değerlendir. Özellikleri tek tek olduğu kadar birlikte de incele: örneğin planlama → kod araması → düzenleme → terminal testi → tarayıcı doğrulaması zincirinin kendi modellerimizle tamamlanabilmesi önemlidir.

**Tercih ilkesi:** Bir müdahale bazı yetenekleri açıp başka bir yeteneği bozuyorsa önce birlikte çalışabilmelerini araştır. Çözülemeyen çatışmada hangi iş akışlarının kazanıldığını ve kaybedildiğini kullanıcıya açıkla; daha faydalı seçeneği ve gerekirse vazgeçilecek özelliği kullanıcı belirler.

## Patch İlkesi

**Varsayılan ve hedef: Cursor'un hiçbir program dosyasına yazılmaz.** Cursor'un protokolü, yerel yürütücüleri, izinleri ve ayarları üzerinden çalış; gerekli uyarlamayı Nexusor tarafında uygula.

**İstisna — kesin zorunluluk.** Yalnız aşağıdaki dört koşulun **tamamı** sağlanırsa patch kabul edilebilir:

1. **Kanıt:** Nexusor tarafında (protokol uyarlaması, ayar/proxy yönetimi, MCP, haricî araç) çözümün **yapılamadığı somut olarak gösterilmiş** olmalı. "Daha zor", "daha fazla kod" veya "temiz görünmüyor" yeterli gerekçe değildir.
2. **Dar kapsam:** Yalnız eksik olan kısmı değiştirmeli; hiçbir değişiklik genişletilmemeli, mevcut bir patch büyütülmemeli ve yeni bir patch yeni bir alan açmamalı.
3. **Geri alınabilirlik:** Hash sabitli olmalı (yalnız doğrulanmış sürüm), tek çapa denetimli olmalı, `-Check`/`-Restore` sunmalı, atomik yazmalı, Cursor çalışırken uygulanmamalı ve sürüm değiştiğinde kendini reddetmeli.
4. **Bildirim:** Kullanıcıya kazanılan işlevi, kaybedilen işlevi ve patch'siz alternatifin neden yetmediğini açıkça sun. Kullanıcı vazgeçmeyi seçebilir.

**Yasak olanlar:** Disk patch'i yerine CDP/çalışma zamanı kod enjeksiyonunu eşdeğer çözüm olarak sunmak. Mevcut patch'i genişletmek. Temiz kurulumda eski değiştirilmiş dosyayı geri kopyalamak. Kurulum paketine, kullanıcı istemediği hâlde patch betiği dağıtmak. Aynı işi iki kez farklı yoldan yapıp belirsizlik bırakmak.

**Patch envanteri kayıt altındadır:** Her aktif patch için `STATUS.md` → "Patch envanteri"
bölümünde hedef dosya, hash, neyi çözdüğü ve patch'siz alternatifinin durumu yazılıdır.
Aktif patch yoksa envanter açıkça boş olarak belirtilir.

## Temiz kurulum ve inceleme sınırı

- **Kullanıcı yapar, ajan başlatmaz.** Cursor kaldırma/temiz kurulum işini kullanıcı yapar; ajan kendiliğinden başlatmaz, profili veya hesabı silmez. Kurulumdan sonra yeni sürümün özgün dosyalarını ve güncel protokolü incele; Nexusor düzeltmelerini patch'siz Cursor üzerinde test et. Önceki patch'li canlı başarıları temiz kurulum kabulü sayma.
- **İnceleme serbest, kurulu dosyalara yazma yasak.** Cursor dosyalarını/protokolünü ayrıntılı ve salt okunur inceleyebilirsin. Gerekirse kurulum dışındaki bir analiz dizinine kopyala, kopyaları aç/biçimlendir ve izole deney yap. İnceleme kopyalarını kurulu Cursor'a geri yazma veya çalışan uygulamaya yükleme. Normal UI test otomasyonu ile uygulama davranışını değiştiren enjeksiyonu ayır.
- **Proxy/CA ve kullanıcı ayarları ayrı değerlendirilir.** Normal proxy, güvenilen CA ve kullanıcı/proje yapılandırması program dosyası patch'i değildir; kullanıcı bunları kaldırmak istemedi. Etkisini ve geri alınmasını ayrıca doğrula.
- **Temiz kurulumdan önce tam altyapı taraması:** Mevcut müdahaleler ve haricî yardımcılar dâhil bütün özellikleri yeniden incele; Cursor'un kendi yoluyla karşılanabilenleri, uyarlama gerektirenleri ve haricî desteği kanıtlı olarak zorunlu olanları raporla. Çalışan bir yardımcıyı yalnız haricî olduğu için kaldırma; Cursor karşılığının işlev/uyumluluk kazanım-kayıp analizi ve doğrulamasıyla karar ver.

## Uyumluluk kararları

- Başarı ölçütü hesabın Free veya Ultra görünmesi değil, kendi modellerimizle gerçekten kullanılabilen toplam işlevdir.
- Gerçek hesap kimliği/oturum, istemcideki plan ve özellik görünümü, model yönlendirmesi ve sunucu tarafındaki hizmet erişimini ayrı değerlendir. Tek bir entegrasyon anahtarıyla hepsini aynı davranışa bağlama.
- Yerel Ultra görünümü veya özellik ayarı kendi modellerimizle kullanılabilen yetenekleri açıyorsa faydasını korumayı hedefle. Sırf resmî Free yanıtıyla aynı olsun diye bu yetenekleri kapatma.
- Yerel Ultra görünümünün sunucu tarafında abonelik veya yetki kazandırdığını varsayma. İstemci görünürlüğü, protokol desteği, model/araç çalışması ve sunucu yetkisi farklı şeylerdir.
- Çevrim içi katalogları, hesap bilgilerini veya araç hizmetlerini gereksiz yere boş/sahte yanıtlarla değiştirme. Her müdahalenin hangi yeteneği açtığını ve hangisini etkilediğini araştır.
- Önce özellikleri birlikte çalıştırmanın yolunu ara. Gerçek bir çatışmada kazanılan özellikleri, bozulan/kaybedilen özelliği, kanıtı ve önerini kullanıcıya sun. Kullanıcı daha faydalı seçeneği tercih edip başka bir özellikten vazgeçebilir; bu tercihi onun yerine sessizce yapma.
- Sadece arayüzde görünmesi veya otomatik testlerin geçmesi, bir özelliğin uçtan uca çalıştığının kanıtı değildir. Kaynak kodu bulgularını, yerel test sonuçlarını ve canlı doğrulamayı ayrı belirt.

## Uygulama ilkeleri

- **Tek yapılandırma noktası Nexusor:** Kullanıcı mümkün olan bütün model, sağlayıcı ve entegrasyon yapılandırmasını yalnız Nexusor üzerinden yapabilmelidir. Nexusor'un güvenilir biçimde yönetebildiği ayarı kullanıcıdan ayrıca Cursor içinde değiştirmesini isteme. BYOK için gereken normal Cursor kullanıcı ayarlarını etkinleştirme, önceki değerleri koruma, gerekiyorsa yeniden yükleme ve sonucu doğrulama Nexusor'un entegrasyon akışının sorumluluğudur. Önce mevcut desteklenen ayar/protokol yollarını araştır ve otomasyonu Nexusor tarafında uygula; yalnız elle yapılmasının gerçekten zorunlu olduğu kanıtlanan adımları nedeni ile birlikte kullanıcıya bırak. Gerçek hesap girişi, OAuth veya işletim sisteminin zorunlu kullanıcı onayı gibi etkileşimleri yetkisizce atlama.
- **Asgari ve geri alınabilir yapılandırma:** Yalnız işlev için gereken kullanıcı/proje ayarlarını yönet; önceki değerleri sakla, sonraki kullanıcı değişikliklerini ezme ve entegrasyon kapatıldığında güvenli geri dönüş sağla. **Yazılan her anahtarın bir karşılığı silinmelidir**; açma ve kapatma simetrik olmalıdır. Arayüzde açık görünmesini başarı sayma; ayarın çalışan Cursor'a yansımasını ve kendi modellerimizle gerçek işlevi ayrı doğrula.
- **Önce mevcut Cursor altyapısını kullan:** Önce kurulu Cursor'un yeteneğini, istemci davranışını, protokolünü ve yerel yürütücüsünü araştır. Mümkünse bu altyapı üzerinden kendi modellerimizle çalıştır; mevcut işlevin yerine paralel bir araç veya ajan sistemi kurma. Yalnız mevcut altyapının neden yetmediği kanıtlandığında eksik kısmı minimum haricî/Nexusor desteğiyle tamamla. Sadece Cursor arayüzünden çağrılması, yürütmenin Cursor altyapısında yapıldığı anlamına gelmez; istemci, protokol, yürütücü ve sağlayıcı yönlendirmesini ayrı belirt.
- Projenin amacını koru; minimum gerekli değişiklikle ilerle, gereksiz kod veya kapsam dışı refactor ekleme.
- Kurulu Cursor sürümünün istemci davranışı ve protokolüyle karşılaştırarak karar ver; önceki varsayımları güncel gerçek gibi kabul etme.
- Sürekli release/kurulum build'i alma. Paketi gerçekten çalıştırıp test edeceğin veya kullanıcının özellikle istediği aşamada derle; kaynak kontrollerini paket derlemesinden ayrı tut.
- Düzeltmeleri hedefli kontrollerle doğrula. Canlı hesap veya çalışan uygulama üzerinde yapılmamış doğrulamaları açıkça belirt.
- Kaynak değişikliği, executable/kurulum derlemesi ve çalışan sürüm birbirinden farklıdır; teslimde hangisinin güncellendiğini belirt.
- Yeni bulguları ve tamamlanan işleri ilgili proje raporuna kaydet. Bu dosyayı kalıcı amaç/karar ilkeleri için, raporları değişen teknik durum için kullan.

## Devralırken dikkat

Teknik geçmiş ve tarihsel kararlar rapor belgelerindedir; bu dosya onların yerine geçmez. **Güncel teknik durum için `STATUS.md` tek kaynaktır.** Yapılacak işler için `ACTIONABLE_BACKLOG.md`, çağrı kimlikli kanıt geçmişi için `PROJECT_REVIEW.md` okunur.

Tarihsel notlar bağlayıcı değildir. Raporda geçen "patch uygulanmalı", "son çare patch" veya tersi ifadeler, Patch İlkesi'nin güncel kuralıyla karşılaştırılmadan talimat sayılmaz. Belgede geçen model adları, özellik listeleri veya kabul ifadeleri de güncel gerçek olduğu varsayılmaz; kod ve canlı kanıtla doğrulanmadan kendini doğrulamış sayılmaz.