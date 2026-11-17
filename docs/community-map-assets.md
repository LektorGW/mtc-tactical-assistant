# Zasoby map społeczności — cache 0.4–0.5

`mtc-assets` pobiera wyłącznie znane obrazy z publicznego serwera kalkulatora społeczności. Pobieranie jest jawną operacją konfiguracji, uruchamianą na żądanie. Moduł nie obserwuje gry, nie zapisuje jej klatek i nie uruchamia sieci podczas odczytu HUD. Pobranie pliku nie jest potwierdzeniem zgodności mapy, skali ani terenu z aktualnym MTC.

## Źródła i konwencje

Katalog jest przypięty do commitu [`39a7af7689524e57ca2326f3b998cf123df95426`](https://github.com/grand-hawk/artillery-calculator/tree/39a7af7689524e57ca2326f3b998cf123df95426). [images.ts](https://github.com/grand-hawk/artillery-calculator/blob/39a7af7689524e57ca2326f3b998cf123df95426/packages/mtc-artillery/src/utils/images.ts) tworzy adresy:

```text
https://maps.artillery-calculator.com/{slug(key)}/user.png
https://maps.artillery-calculator.com/{slug(key)}/heightmap.png
```

Źródłowy `slug` dla przejrzanych kluczy ASCII usuwa apostrof, zamienia spacje na myślniki i zmniejsza litery. Nie wyprowadzamy adresu z dowolnego tekstu OCR. Publiczna funkcja synchronizacji przyjmuje ID ze wbudowanego katalogu, a nie URL. [maps.ts](https://github.com/grand-hawk/artillery-calculator/blob/39a7af7689524e57ca2326f3b998cf123df95426/packages/mtc-artillery/src/config/maps.ts) podaje 25 pełnych wymiarów i 21 zakresów heightmap; cztery wpisy bez zakresu nie dostają fikcyjnej wysokości.

[useHeightmapZ.ts](https://github.com/grand-hawk/artillery-calculator/blob/39a7af7689524e57ca2326f3b998cf123df95426/packages/mtc-artillery/src/hooks/data/useHeightmapZ.ts) odczytuje kanał czerwony obrazu i interpoluje liniowo pomiędzy wartościami dla 0 i 255. Zakres źródłowy jest w studs. [useHeightDifference.ts](https://github.com/grand-hawk/artillery-calculator/blob/39a7af7689524e57ca2326f3b998cf123df95426/packages/mtc-artillery/src/hooks/data/useHeightDifference.ts) stosuje przelicznik z [math.ts](https://github.com/grand-hawk/artillery-calculator/blob/39a7af7689524e57ca2326f3b998cf123df95426/packages/mtc-artillery/src/utils/math.ts): `1 stud = 0.36 game_meters`. Katalog `KnownMapResource` udostępnia już przeliczone końce zakresu.

[Canvas.tsx](https://github.com/grand-hawk/artillery-calculator/blob/39a7af7689524e57ca2326f3b998cf123df95426/packages/mtc-artillery/src/components/templates/Canvas.tsx) i [HeightmapProvider.tsx](https://github.com/grand-hawk/artillery-calculator/blob/39a7af7689524e57ca2326f3b998cf123df95426/packages/mtc-artillery/src/components/providers/HeightmapProvider.tsx) stosują wspólne znormalizowane XY, bez jawnego odwrócenia osi heightmap. Azymut źródłowy `90° + atan2(dy, dx)` oznacza północ u góry i wschód po prawej. Jest to konwencja kalkulatora, nie niezależne sprawdzenie georeferencji aktualnych rastrów.

Źródłowa heightmap może być prostokątna przy kwadratowym obrazie mapy. Przykłady: Dustbowl 2972×3442, Rohkshort 4096×2377, Waterloo 4096×2048. Tekstura `user.png` jest kwadratem z marginesami, a siatka 9×9 obejmuje cały kwadrat. Od 0.5 heightmapa jest wyśrodkowana i dopasowana dłuższym bokiem do tego kwadratu (letterbox), a nie rozciągana; marginesy to brak danych. Tak samo postępuje fork [DJDeDPon/mtc-artillery-calculator-extended](https://github.com/DJDeDPon/mtc-artillery-calculator-extended) (`drawLetterboxedHeightmap`). Przypięty kalkulator źródłowy rozciąga obraz; zgodność obu wariantów z grą nie została zmierzona. Dopasowanie geometrii widocznej mapy oraz świadome przyjęcie tej konwencji pozostają wymagane przed obliczeniami. Heightmap opisuje powierzchnię terenu; nie podaje wysokości celu powietrznego ani pojazdu nad terenem.

## Licencja i dystrybucja

[Licencja repozytorium](https://github.com/grand-hawk/artillery-calculator/blob/39a7af7689524e57ca2326f3b998cf123df95426/LICENSE.md) to MIT, `Copyright (c) 2024 Astrid`. W przejrzanych plikach nie znaleziono osobnego oznaczenia licencji rastrów na oddzielnym serwerze. `sourceMetadata.rasterLicense` ma wartość `unconfirmed`, a `redistributionAllowed` — `false`. To oznaczenie ogranicza dystrybucję przez tę aplikację; nie jest opinią prawną o wszystkich możliwych zastosowaniach.

Zasoby pobieramy do oddzielnego cache użytkownika na potrzeby aplikacji. Nie dołączamy plików `user.png`, `heightmap.png` ani ich pochodnych do `.exe`, instalatora lub ZIP. Sam katalog nazw, adresów, wymiarów i źródeł pozostaje w kodzie. Potwierdzenie źródła może być globalne dla tej biblioteki; nie zmienia stanu `experimental` na `verified`.

## API i dane

```rust
let catalog = mtc_assets::known_maps()?;
let cache = mtc_assets::AssetCache::new(user_cache_path, mtc_assets::CacheLimits::default())?;
let result = cache.sync(&selected_map_ids, true, true, mtc_assets::SyncMode::Online)?;
let map = cache.validate_cached("arctic_airbase", mtc_assets::AssetKind::MapImage)?;
let terrain = cache.prepare_heightmap("arctic_airbase", 1024)?;
```

`KnownMapResource` używa JSON camelCase: `schemaVersion`, `id`, `name`, `key`, `mapUrl`, opcjonalne `heightmapUrl` i `heightRangeGameMeters { minimum, maximum }`, `sizeGameMeters`, `unit`, `status`, `sourceMetadata`. Metadata zawiera repozytorium, commit, adresy przejrzanych źródeł, dwie informacje o licencji, ograniczenie dystrybucji i opis założenia o wyrównaniu osi.

`AssetKind`: `mapImage`, `heightmap`, `preparedHeightmap`. `SyncMode`: `online`, `offline`. `SyncReport` zawiera `schemaVersion`, `mode`, `completedAtMs`, `results`. Każdy `AssetSyncResult` ma `id`, `kind`, `status`, opcjonalny `cached`, `changed` i opcjonalny błąd `{ code, message }`. Stany synchronizacji: `downloaded`, `cached`, `missing`, `failed`.

`CachedRaster` podaje lokalną ścieżkę, SHA-256, rzeczywiste wymiary, długość pliku, czas pobrania, oryginalny URL i źródła. Dla pochodnego pliku dochodzą `sourceSha256` i `processing`. Zmiana hasha przy ponownym pobraniu daje `changed: true`; konsument musi wtedy unieważnić kalibrację i pomiary powiązane ze starym hashem. Adresy rastrów nie są wersjonowane commitem GitHub: zapisany hash identyfikuje faktycznie pobraną zawartość.

`sync_with_progress` przekazuje wynik każdego zasobu do callbacku. Callback nie powinien ponownie wywoływać metod tego samego cache, bo operacja jest chroniona jednym mutexem. Wywołania sieciowe i dekodowanie należy umieścić w `spawn_blocking` w aplikacji Tauri. Przed synchronizacją aplikacja powinna zatrzymać odczyt gry; sama biblioteka nie zna stanu UI.

Tryb `online` pobiera wskazane zasoby ponownie. Błąd sieci lub serwera daje jawny wynik błędu, bez milczącego zastąpienia go starszą wersją. Tryb `offline` nie tworzy klienta sieciowego: sprawdza lokalne rekordy, SHA-256, długość, format i wymiary. Brak pliku daje `missing`. Nieznane ID, powtórzone żądania i brak potwierdzenia źródła są odrzucane przed dostępem do sieci.

## Limity i zapis atomowy

Domyślne `CacheLimits`:

| Pole | Limit |
|---|---:|
| `maxBodyBytes` | 40 MiB na zasób |
| `maxDimension` | 8192 px |
| `maxPixels` | 64 Mi pikseli |
| `maxDecodeBytes` | 320 MiB dla dekodera |
| `maxCacheBytes` | 512 MiB |
| `timeoutSeconds` | 30 s na żądanie |
| `connectTimeoutSeconds` | 10 s |

Adres musi być dokładnym HTTPS URL z katalogu na `maps.artillery-calculator.com`. Inny host, port, dane logowania, query, fragment i ścieżka są odrzucane. Przekierowania i proxy z konfiguracji środowiska są wyłączone; weryfikacja certyfikatu TLS pozostaje aktywna. Wymagamy `image/png`, 8-bitowego PNG i poprawnego dekodowania. Limit bajtów jest sprawdzany także dla strumienia bez zadeklarowanej długości.

Cache przechowuje bloby adresowane zawartością `blobs/{sha256}.png` i małe indeksy `entries/{id}-{kind}.json`. Zapis trafia do pliku tymczasowego w tym samym katalogu, jest synchronizowany i atomowo zatwierdzany. Indeks zmienia się dopiero po zapisie poprawnego bloba. Ścieżka bloba wynika z hasha, nie z pola ścieżki w zewnętrznym JSON. Wyjście poza katalog, dowiązania zamiast plików, nadmierne indeksy i niespójne metadane są blokowane. Świeży, odkodowany download może atomowo przywrócić uszkodzony zwykły blob; odczyt offline zgłasza uszkodzenie. Stare bloby pozostają po odświeżeniu; po osiągnięciu limitu synchronizacja zgłasza brak miejsca zamiast niejawnego kasowania danych.

## Przygotowanie dużych heightmap

`prepare_heightmap(id, maxDimension)` przyjmuje 32–2048 pikseli na dłuższej osi, domyślnie integracja używa 1024. Zachowuje proporcje rastra i odczytuje kanał R zgodnie ze źródłem. Tworzy PNG z równymi kanałami RGB, który pasuje do importu `HeightMap` z modułu map. Każda próbka jest wyznaczona biliniowo; nie skalujemy kolorów przez luminancję. Przezroczysty lub częściowo przezroczysty punkt o niezerowej wadze daje brak danych, nie wysokość zero.

Ta transformacja ma jawną wersję `red8-bilinear-contain-square-missing-v2`: wynik jest kwadratem o boku `min(maxDimension, dłuższy bok)`. Pliki v1 (rozciągnięte) są odrzucane i przygotowywane ponownie. Wpis terenu zapisuje wersję w polu `source`; tryb katalogu nie używa starszego terenu. Źródło używa najbliższej próbki z `Math.round`, a aplikacja stosuje bilinear sampling oraz końce `0..width−1` / `0..height−1` i odrzuca brakujące próbki. To konserwatywna adaptacja, nie identyczność algorytmu. Zmniejszenie rozdzielczości może pominąć wąskie szczegóły terenu. Kodowanie pozostaje 8-bitowe, z krokiem wysokości `(maximum−minimum)/255`.

Przy każdej walidacji pochodnego pliku sprawdzany jest aktualny hash surowego źródła. Zmieniona lub brakująca raw heightmap blokuje pochodny plik. Transformacja nie potwierdza orientacji, pochodzenia wymiarów ani bieżącej wersji mapy.

## Komenda `prepare_catalog_map` (0.5)

Wejście: `{ id, includeTerrain, allowOnline, sourceAcknowledged }`. Wymaga `sourceAcknowledged: true` i ID z katalogu. Kolejność: sprawdzony cache offline; przy braku mapy lub terenu i `allowOnline` — pobranie przypiętego URL. Wynik dla każdego rodzaju pliku pochodzi z pierwszego udanego źródła, więc nieudane pobranie terenu nie usuwa mapy z cache.

Wyjście: `{ map: StoredMap, terrain: StoredTerrain | null, terrainInSource, origin: "cache" | "downloaded", sizeGameMeters, warnings }`. `map.calibration`: `unitsPerPixel = sizeGameMeters / width`, obrót 0°, początek 0. Brak mapy daje błąd z informacją o ręcznym imporcie. Brak terenu w źródle (Gensokyo, Japan, Reactor, Snow Tundra) daje ostrzeżenie, nie zerową wysokość.

Uwaga dla środowisk z wirtualizacją MSIX: proces potomny aplikacji ze sklepu, np. narzędzia deweloperskiego, zapisuje `%LOCALAPPDATA%` w `…\Packages\<pakiet>\LocalCache\Local`. Ścieżki zapisane w bibliotece nadal działają, ale zwykłe uruchomienie aplikacji widzi pusty cache. Tryb katalogu najpierw używa zweryfikowanego wpisu biblioteki, a następnie pobiera brakujące pliki do właściwego cache.

## Polecenia i weryfikacja

Z katalogu głównego projektu:

```powershell
cargo test -p mtc-assets
cargo clippy -p mtc-assets --all-targets -- -D warnings
cargo fmt -p mtc-assets --check
cargo run -p mtc-assets --example sync_known_maps -- --all --terrain --prepare --ack
cargo run -p mtc-assets --example sync_known_maps -- --all --terrain --prepare --ack --offline
```

CLI domyślnie wybiera `%LOCALAPPDATA%/MTC-Tactical-Assistant/community-assets-v1`. `--cache <path>` wskazuje inny katalog, `--map <known-id>` ogranicza synchronizację do jednej mapy, a `--output <path>` zapisuje raport JSON. `--ack` odpowiada jawnemu przyjęciu eksperymentalnego źródła, nie potwierdzeniu parametrów w rozgrywce. Bez `--terrain` pobierane są wyłącznie obrazy map.

Testy jednostkowe używają oryginalnych syntetycznych PNG, nie sieci. Sprawdzają katalog, ograniczenie URL, zgody, brak danych offline, hash i geometrię, limity strumienia/dekodera/cache, atomowe zastąpienie indeksu oraz czerwony kanał, brakujące próbki i zależność pochodnego pliku od źródła. Rzeczywisty synchronizator raportuje wyniki każdego zasobu i ich SHA-256. Dostępność serwera w chwili testu nie oznacza trwałej dostępności lub zgodności z aktualnym MTC.

Przeprowadzony pełny sync pobrał i odkodował 25/25 map oraz 21/21 deklarowanych heightmap, przygotowując 21 pochodnych PNG bez błędów. Odczyt offline zweryfikował następnie 46/46 surowych plików z identycznymi SHA-256; dla 21 pochodnych sprawdzono zgodność zapisanego hasha źródłowego. Raporty znajdują się w `tests/output/v0.4.0/assets-sync.json`, `assets-offline.json` i `assets-verification.json`, a skrót rozmiarów w `assets-summary.json`. Surowe pliki zajęły 167 679 374 B, pochodne — 11 841 726 B, razem około 171,2 MiB. Dane pozostają w oddzielnym cache użytkownika. Zmienna `%LOCALAPPDATA%` może być przekierowana przez środowisko aplikacji Store; rzeczywistą kanoniczną ścieżkę zapisuje pole `cacheRoot` raportu.
