# Arşivlenmiş Cursor patch betikleri — UYGULANMAZ

Bu klasördeki betikler **artık uygulanmaz, paketlenmez ve Nexusor tarafından çağrılmaz.**

Projenin hedefi: Cursor'un hiçbir program dosyasına müdahale etmeden bütün özelliklerini
Nexusor üzerinden yönetilebilir kılmak. Patch yalnız "kesin zorunluluk" varsa kabul edilir
(`AGENTS.md` → Patch İlkesi).

## Durum

| Betik | Özellik | Yerine geçen çözüm |
|---|---|---|
| `fix-cursor-child-stop.ps1` | Alt ajan kartındaki Stop düğmesi | Nexusor yerel görev paneli (`stop_run` / `stop_tool`) — **çalışıyor, canlı doğrulandı** |
| `fix-cursor-background.ps1` | "Run in background" düğmesi | Nexusor yerel görev paneli (`background_tool`) — **çalışıyor, canlı doğrulandı** |
| `fix-cursor-image-write.ps1` | Binary Write'a atomik `O_EXCL` | Nexusor'un kendi `create_new(true)` yazımı + Cursor izin kapısı — **kaynak değiştirildi** |

Üçü de kaldırıldı: kurulum paketine gömülmezler (`tauri.conf.json` `bundle.resources`
yalnız `setup-media.ps1`, `FIRST_RUN.md` ve `STATUS.md` içerir) ve hiçbir kod yolu onları
tetiklemez.

## Mevcut patch'li kurulumlar için tek amaç: geri alma

Bu betikler `-Restore` destekler. Eğer Cursor kurulumun daha önce patch'lendiyse ve özgün
dosyalara dönmek istiyorsan:

```powershell
pwsh -File scripts\legacy\fix-cursor-image-write.ps1 -Check      # durumu oku
pwsh -File scripts\legacy\fix-cursor-image-write.ps1 -Restore    # özgün dosyaya dön
```

**Cursor çalışırken çalıştırma.** Betik hash sabitlidir ve yalnız doğrulanmış Cursor
sürümünde çalışır; sürüm değiştiyse kendini reddeder ve hiçbir şeye dokunmaz.

## Kurallar

- **Yeni patch ekleme, genişletme veya yeniden uygulama.**
- **Temiz kurulumda hiçbir betiği çalıştırma.** Temiz kurulum Cursor'a hiç dokunmadan
  çalışacaktır.
- Bu klasör bir referans arşividir, bir dağıtım kanalı değildir.