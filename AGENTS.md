# Lazarbyte: Proje Yönergeleri

Kod, yorumlar, commit mesajları ve dokümantasyon İngilizce yazılır.
Benimle (proje sahibi) Türkçe konuş.

## 1. Proje nedir
Lazarbyte, Android için dosya kurtarma uygulaması. Rust-first: tüm çekirdek
mantık Rust'ta, Kotlin (Jetpack Compose) yalnızca arayüz, izinler ve Android
API entegrasyonu. Rust-Kotlin köprüsü uniffi-rs, Android derlemesi cargo-ndk.

Bu repo, sltcnb/breadcrumb-rs (MIT) projesinin fork'u. breadcrumb-rs; imza
tabanlı carving (28 tür), FAT/exFAT/ext2-3-4/NTFS/HFS+/APFS undelete, doğrulama
(--validate), tarama devam ettirme (--resume), fuzz testleri ve manifest
üretimi olan bir adli bilişim CLI aracı. Bağımlılıkları: aho-corasick, sha2,
flate2 (rust_backend). Hepsi saf Rust, C toolchain gerekmiyor. Bunu koru.

Hedef: bu çekirdeği Android'e taşımak ve üstüne mobil odaklı özellikler eklemek.

## 2. Lisans kuralları (kesin, istisnasız)
- Projemiz Apache-2.0. Upstream kod MIT: upstream'den gelen dosyalardaki MIT
  telif bildirimleri korunur ve THIRD_PARTY_LICENSES.md içinde belgelenir.
  LICENSE (Apache-2.0) ve NOTICE dosyalarını oluştur.
- İSTİSNA: MPL-2.0 yalnızca DEĞİŞTİRİLMEMİŞ bağımlılıklar için ve yalnızca
  uniffi ailesi (uniffi, uniffi_core, uniffi_macros, uniffi_meta, uniffi_udl,
  uniffi_build, uniffi_bindgen ve bunların getirdiği MPL-2.0 crate'ler) için
  izinlidir. Gerekçe: MPL-2.0 dosya bazlı zayıf copyleft; bağımlılık olarak
  kullanmak kendi kodumuzu MPL yapmaz. uniffi dosyalarını asla vendor'lama,
  yamalama veya değiştirme. İstisna deny.toml'de yalnızca bu crate'ler için
  tanımlanır ve THIRD_PARTY_LICENSES.md'de belgelenir.
- GPL/AGPL/LGPL kodunu (PhotoRec, TestDisk, Sleuth Kit, Scalpel-GPL türevleri
  vb.) kopyalama, çevirme, uyarlama. Davranışı anlamak için okuman bile gerekirse
  bana sor, kod yazma.
- Ticari ürünlerin tersine mühendisliğinden çıkan kod/doküman kullanma.
- Yeni bağımlılık eklemeden önce lisansını ve ne işe yaradığını bana yaz.
  Sadece MIT, Apache-2.0, BSD, ISC, Unlicense, Zlib kabul. cargo-deny'ı
  yapılandır (deny.toml) ve her bağımlılık değişikliğinde çalıştır.
- Başka yerden kod uyarlarsan kaynağı ve lisansını yorumda belirt.

## 3. Veri güvenliği kuralları (kesin)
- Kaynak depolama/imaj ASLA yazılabilir açılmaz. Kaynak her zaman salt okunur.
  Kurtarma çıktısı daima kaynaktan FARKLI bir hedefe yazılır. Bunu kodda
  zorunlu kıl (tip sistemiyle veya doğrulamayla), kullanıcı ayarına bırakma.
- Kullanıcı verisini işleyen çekirdek kodda panic olmaz: unwrap/expect,
  dizi indeksi taşması, aritmetik taşma yasak. Hepsi Result döner. Offset ve
  boyut hesaplarında checked_* kullan. Bozuk/kötü niyetli girdi uygulamayı
  çökertmemeli ve sonsuz döngüye sokmamalı.
- Çekirdek crate'lerde #![forbid(unsafe_code)]. Unsafe sadece FFI/fd sınırında
  gerekirse, ayrı modülde, gerekçe yorumuyla ve benim onayımla. Onaylanan tek
  istisna: lazarbyte-ffi içindeki izole FdSource modülü (bkz. §10).
- Çıktı yazarken hedefte boş alan kontrolü yap (upstream'deki --min-free
  mantığı korunsun). Disk dolduran tarama olmasın.
- Uygulama hiçbir kullanıcı dosyasını ağa göndermez. AndroidManifest'e
  INTERNET izni EKLEME. Analitik/telemetri/reklam SDK'sı yok.

## 4. Doğruluk ve dürüstlük kuralları
- Var olduğundan emin olmadığın crate, API, flag veya fonksiyonu uydurma.
  Emin değilsen "emin değilim" de; ben doğrularım ya da sen dokümandan kontrol et.
  Sürüm numaralarını ve Android/uniffi/cargo-ndk API'lerini ezberden yazma,
  resmi dokümandan doğrula.
- Kurtarılan dosya için sonucu daima güven seviyesiyle raporla (verified /
  failed / unvalidated / low confidence). Hiçbir yerde (kod, UI, README)
  "kesin kurtarıldı" veya "her şeyi kurtarır" deme.
- Bilmediğin veya varsaydığın şeyi açıkça belirt. Test etmediğin kod için
  "çalışıyor" deme.
- Android'in dahili depolaması FBE + TRIM nedeniyle büyük ölçüde kurtarılamaz.
  README ve UI bunu dürüstçe söylemeli. Gerçekçi kapsam: SD kart/USB OTG
  (FAT/exFAT), disk imajı dosyaları, MediaStore çöp kutusu, uygulama
  artıkları ve önleme (silmeden önce yakalama).

## 5. Hedef mimari
Cargo workspace:
- lazarbyte-core: saf kütüphane. Carving, validate, FAT/exFAT/ext undelete.
  Android'e bağımlı kod yok. CLI ve dosya yolu mantığı burada OLMAZ.
- lazarbyte-cli: ince kabuk (masaüstünde test/karşılaştırma için).
  Upstream'in CLI davranışı korunur. Ürün değildir; bkz. §10.
- lazarbyte-ffi: uniffi bağlamaları (UDL veya proc-macro; güncel resmi öneriyi doğrula).
- android/: Kotlin + Compose uygulaması.

Core'da yapılacak soyutlamalar:
- Source trait: Read + Seek + uzunluk (dosya yolu, ham fd ve bellek için
  implementasyonlar). Android SAF/USB OTG bize yol değil file descriptor verir.
- Sink trait: çıktıyı klasöre yazmak yerine soyut hedefe (Android'de SAF
  fd'leri). Manifest üretimi korunur.
- İlerleme (progress) ve iptal (cancellation) callback'leri.
- Ağır/adli formatlar Cargo feature flag arkasına alınır: ewf, bitlocker,
  apfs, hfs, ntfs. Mobil build'in varsayılanı: carving + fat/exfat + ext.
  Hedef: APK küçük kalsın, bakım yükü azalsın.
- Tarama kaynak kullanımı ayarlanabilir: thread sayısı ve chunk boyutu
  (mobilde varsayılan düşük; 32 MiB / tüm çekirdek varsayılanı telefonda uygun değil).

Android tarafı:
- Hedef ABI: arm64-v8a ve x86_64 (emülatör). Native kütüphaneler Google Play'in
  16 KB sayfa boyutu gereksinimine uymalı (güncel gereksinimi doğrula).
- Depolama: Storage Access Framework ve MediaStore. MANAGE_EXTERNAL_STORAGE
  mümkünse kullanma (Play politikası); gerekiyorsa önce bana gerekçe sun.
- Uzun taramalar foreground service içinde; ısınma/pil için kısıtlama.
- Kotlin'de iş mantığı yok. Çekirdek kararları Rust'ta.

## 6. Test ve doğrulama
- Altın kural: aynı imajı masaüstü CLI ve FFI sürümüyle tara; manifest
  kayıtları (offset, size, sha256, ext, validated) BİREBİR aynı olmalı.
  Upstream'in byte-parity yaklaşımını sürdür.
- Her carving/ayrıştırma değişikliği için test: bilinen FAT32/exFAT imajı
  oluştur, dosya koy, sil, kurtar, SHA-256 karşılaştır.
- Mevcut fuzz_smoke testlerini ve fuzz/ hedeflerini bozma, yeni ayrıştırıcılara
  fuzz hedefi ekle. Çöken girdiler regresyon korpusuna girer.
- Her adım sonunda şunları çalıştır ve ÇIKTIYI raporla:
  cargo fmt --check, cargo clippy --all-targets -- -D warnings, cargo test,
  cargo deny check.
- Android derlemesi için cargo ndk build'i ve emülatör testini raporla.

## 7. Çalışma şekli
- Küçük adımlar: her adım tek konu, tek commit, açıklayıcı mesaj.
- Önce plan yaz ve onayımı bekle, sonra kodla.
- Kapsam dışına çıkma. İstemediğim refactor, yeniden adlandırma, bağımlılık
  güncellemesi yapma.
- Upstream davranışını bozacak değişikliği önce bana göster.
- Her adımın sonunda: ne yaptın, neyi test ettin, neyi test EDEMEDİN, riskler.
- Büyük dosyaları baştan yazma; mevcut kodu anla ve minimal değişiklik yap.

## 8. Yol haritası (sırayla, her faz benim onayımla)
Faz 0: Repo analizi ve rapor (kod yazma).
Faz 1: Workspace'e geçiş (core / cli ayrımı), davranış değişmeden. Mevcut
       testler yeşil kalmalı.
Faz 2: Feature flag'ler, mobil varsayılan profil, boyut ölçümü.
Faz 3: Source/Sink trait'leri, progress/cancel; CLI hâlâ aynı sonucu vermeli.
       Zorunlu içerik ve sıra §10'da.
Faz 4: uniffi FFI crate'i, masaüstünde FFI üzerinden parity testi.
Faz 5: cargo-ndk ile Android derlemesi, emülatörde duman testi.
Faz 6: Kotlin/Compose MVP: kaynak seçimi (SAF/OTG), hızlı/derin tarama,
       ilerleme, thumbnail önizleme, tür filtresi, seçip kurtarma.
Faz 7: MediaStore çöp kutusu ve "önleme" (silmeden önce yakalama).
Faz 8: Dürüst README, Play Store hazırlığı (izin gerekçeleri, gizlilik politikası).

## 9. İlk görev (Faz 0): KOD YAZMA, sadece raporla
Repoyu incele ve bana şunları raporla:
1. src/ modül haritası ve her modülün sorumluluğu.
2. Hangi yerlerin std::fs::File / dosya yolu / CLI'ya bağlı olduğu, saf
   çekirdek mantıkla CLI kodunun nerede karıştığı.
3. Android'de çalışmayacak veya riskli her şey (mmap, ham cihaz açma, /dev
   erişimi, platforma özel çağrılar, thread/bellek kullanımı).
4. Feature flag ile ayrılabilecek formatlar ve her birinin yaklaşık kod boyutu.
5. Mevcut test durumu: hangi testler var, hepsi yeşil mi (çalıştırıp raporla).
6. Bağımlılıklar, lisansları, MSRV ve uniffi/cargo-ndk ile uyumluluk sorunları.
7. Önerdiğin Source/Sink trait tasarımı ve Faz 1'in adım adım planı.
8. Emin olmadığın ve benden/dokümandan doğrulanması gereken noktalar.

## 10. Android-only kararı (2026-11-08, proje sahibi)
Ürün SADECE Android'de çalışır. Masaüstü ürünü yoktur; masaüstü yalnızca
geliştirme ve doğrulama ortamıdır.

- lazarbyte-cli bir ürün değil, geliştirme ve parity test aracıdır. Minimal
  tutulur; yeni CLI özelliği eklenmez ve release'e konmaz. Windows'ta
  geliştirildiği için masaüstünde derlenip test edilebilir kalır.
- Mobil varsayılan profil: carving + fat/exfat + ext. Şunlar varsayılan
  KAPALI: ntfs, hfs, apfs, ewf, bitlocker, containers (qcow2/vmdk/stdin),
  grep, customsig, artifacts. Faz 2'de boyut ölçümü yapılır; hiç
  kullanılmayacağı netleşenler silinmeden önce proje sahibine sorulur.
- Masaüstüne özgü kod core'a girmez: artifacts read_dir taraması, stdin
  spool, HTML rapor ve CLI argüman ayrıştırma cli'da kalır veya feature flag
  arkasına alınır.
- CI: ana hedef Linux. Android derlemesi (cargo-ndk) Faz 5'te CI'ya eklenir.
  macOS/Windows matrisi şimdilik korunur ama ağırlık verilmez.
- Android profili panic = "unwind" kullanır (uniffi panikleri catch_unwind ile
  yakalar; panic = "abort" ile yakalayamaz). Çekirdekteki tüm unwrap/expect
  Faz 3'te Result'a çevrilir; bu iş Faz 4'ten ÖNCE biter.
- Faz 3 zorunlu sırası: (a) pread/read_at hata yutmayı düzelt, (b) undelete
  modlarını (fat/ext4/hfs/apfs) komple RAM yerine akışlı yazmaya çevir,
  (c) validate'in 256 MiB RAM tavanını mobil için düşür ve ayarlanabilir yap,
  (d) chunk boyutu ve thread sayısını ayarlanabilir yap; mobil varsayılan düşük.
- FdSource unsafe izni verildi: yalnızca lazarbyte-ffi içinde, izole bir
  modülde, Kotlin'in ParcelFileDescriptor.detachFd() ile verdiği fd'yi
  OwnedFd::from_raw_fd ile sahiplenerek. Modül başında güvenlik sözleşmesi
  yorumu zorunlu (fd geçerli ve tek sahipli). Çekirdekteki
  #![forbid(unsafe_code)] aynen kalır; core'da unsafe yoktur.
- Parity testi: Faz 1'de eski ve yeni ikili aynı imajda aynı JSON'u üretmeli;
  çalıştırmaya göre değişen alanlar (zaman damgası, süre, mutlak yol)
  karşılaştırma dışı bırakılır ve listelenir. Faz 4-5'te aynı kontrol masaüstü
  harness ile emülatördeki FFI çağrısı arasında yapılır.
