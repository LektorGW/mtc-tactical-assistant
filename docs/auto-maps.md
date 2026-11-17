# Automatyczne mapy i teren — 0.3

## Szybki tryb „Mapa z katalogu” (0.5)

Zakładka **Mapa i cel** startuje w trybie **Mapa z katalogu**:

1. Jednorazowo kliknij **Akceptuję źródło i pobieranie map**. Zgoda trafia do `catalogSourceAccepted` w `settings-v4.json`.
2. Wyszukaj mapę (25 pozycji; najpierw 12 map w rotacji wg multicrew.wiki z 2026-10-15) i kliknij ją.
3. Aplikacja używa kolejno: zweryfikowanego wpisu biblioteki (SHA-256), lokalnego cache, pobrania przypiętego adresu. Teksturę i heightmapę pobiera komenda `prepare_catalog_map`.
4. Skala = `sizeGameMeters / szerokość tekstury`, północ = góra, jednostki = metry gry. Tak samo liczy kalkulator źródłowy: `|Δ znormalizowane XY| × size`. Obraz niekwadratowy jest odrzucany.
5. LPM stawia aktywne narzędzie (domyślnie **Działo**, potem **Cel**), PPM zawsze stawia cel. Punkty można przeciągać. Kółko myszy przybliża wokół kursora, przeciąganie mapy ją przesuwa, `+`/`−`/`0`/strzałki działają z klawiatury.
6. Wynik pokazuje sektory działa i celu, dystans, azymut, różnicę wysokości ze źródłem (`terrain`/`manual`) oraz elewację i czas lotu, jeśli profil na to pozwala. Ten sam wynik trafia na nakładkę.

Bez internetu i bez cache pojawia się komunikat oraz przycisk przejścia do ręcznego importu. Ręczny import i kalibracja A–B zostały w trybie **Własny obraz (zaawansowane)**.

Siatka domyślnie ma 9×9 pól na całym obrazie: źródło liczy mapę jako `calculateMapSize(bok_pola, 9)`, a tekstury mają narysowaną siatkę 9×9. Domyślne etykiety — kolumny A B C D E F G H J (bez I), wiersze 1–9 od góry, sektor np. „C7” — są zgodne ze zrzutem minimapy MTC z 2026-10-15. Mogą się zmienić z aktualizacją gry, dlatego liczbę pól, styl kolumn i kierunek wierszy można zmienić. Pole „Rozmiar pola z gry” (lewy dolny róg mapy w grze, klawisz M) służy do porównania ze skalą z katalogu.

`mtc-maps` obsługuje dokładne dopasowanie jawnych nazw, rejestr lokalnych referencji rastrowych, geometryczne dopasowanie widoku do referencji oraz wysokość z zaimportowanej heightmap. Korzysta wyłącznie z przekazanych obrazów i danych. Nie pobiera zasobów gry, nie steruje wejściem, nie zapisuje klatek i nie korzysta z pamięci Roblox.

## Nazwa katalogowa

```rust
use mtc_maps::{match_map_name, MapNameEntry, MapNameStatus};
let catalog = vec![MapNameEntry {
    id: "arctic_airbase".into(),
    name: "Arctic Airbase".into(),
    aliases: vec!["Airbase".into()],
}];
let found = match_map_name("ARCTIC--AIRBASE", &catalog)?;
assert_eq!(found.status, MapNameStatus::Matched);
```

Normalizacja obejmuje wielkość liter, odstępy/znaki interpunkcyjne i polskie diakrytyki. Dopasowanie wymaga całego znormalizowanego napisu: nazwy, jawnego aliasu albo ID. Fragment, pusty napis i nieznana nazwa dają `unknown`; alias wspólny dla dwóch wpisów daje `ambiguous` i puste `mapId`. Nie stosujemy najbliższej nazwy, zgadywania mapy ani przypisywania pierwszego ID z katalogu.

Katalog nazw może pochodzić z lokalnych danych społeczności. Nazwa odczytana przez OCR i identity katalogu nie są dowodem geometrii, skali lub aktualności mapy. Moduł nie zawiera 25 zdalnych tekstur: dla każdego dopasowania obrazu potrzebna jest lokalna referencja.

## Rejestr referencji

`MapReference::new(metadata, &RgbaImage)` wymaga:

- `MapReferenceMetadata { schemaVersion: 1, id, name, unit, source, confirmed, calibration }`;
- dodatniej, skończonej kalibracji `MapCalibration`, właściwej jednostki `game_units` / `game_meters` i jawnego potwierdzenia źródła/geometrii;
- rastra co najmniej 32×32, maksymalnie 16M pikseli, z użyteczną teksturą.

Referencja przechowuje w RAM zredukowaną szarość obrazu (maks. 512 px na dłuższej osi) do deskryptora widoku oraz, od 0.5, indeks cech piramidy do 1024 px liczony raz przy `MapReference::new`. `Clone` współdzieli ją przez `Arc`. Publiczny `MapDescriptor` zawiera ID, rzeczywiste wymiary oryginału, siatkę 16×16 / 256 próbek i RMS tekstury; można zapisać go jako metadane. Sam deskryptor nie wystarcza do dopasowania częściowej mapy.

`MapReferenceRegistry::register` aktualizuje istniejące ID albo dodaje referencję, do limitu 32. `references()` udostępnia referencje, `descriptors()` małe dane do prezentacji/zapisu, `remove(id)` usuwa wpis. Rejestr nie wczytuje plików ani nie zapisuje obrazu. Integracja może zapisać ścieżkę wcześniej zaimportowanego przez użytkownika pliku, a po starcie odczytać go do RAM. Zmiana wymiarów, zawartości, kadru lub osi tego pliku wymaga unieważnienia poprzedniej kalibracji; należy porównać aktualne wymiary z zapisanymi metadanymi.

## Widok częściowy, zoom, obrót i pełna klatka HUD

```rust
use mtc_maps::{match_map_view, MapMatchConfig, MapViewStatus};
let result = match_map_view(&captured_client_frame, registry.references(), &MapMatchConfig::default())?;
if result.status == MapViewStatus::Matched {
    let candidate = result.best.expect("matched has a candidate");
    let reference_point = candidate.reference_point(observed_marker_in_client_pixels)?;
    let world_point = candidate.calibrated_world_point(observed_marker_in_client_pixels)?;
}
```

Matcher działa na całej przekazanej klatce; mapa może być jej fragmentem z HUD/sceną poza mapą. Własny, deterministyczny detektor narożników wybiera punkty przestrzennie, aby mocniejszy HUD nie zajął całego budżetu. Piramida skali i deskryptory 128 bitów porównujące pary jasności dostarczają kandydatów. Deskryptor wykorzystuje otoczenie o promieniu 23 pikseli obrazu roboczego; większy kontekst odróżnia sąsiedztwa dużych, płaskich kafli, które w małym obszarze mogą mieć identyczną kolejność jasności. Geometryczna zgodność jest dopasowywana do podobieństwa 2D: przesunięcia, jednej skali i obrotu. Nie jest to model perspektywy ani semantyczne rozpoznawanie wszystkich map.

Najbliższy deskryptor musi przejść limit odległości i przewagę nad drugim; remis, także odległości 0, jest odrzucany. Punkty i ich odpowiedniki są deduplikowane. Dopasowanie wymaga minimalnej liczby zgodnych punktów, udziału inlierów, błędu reprojekcji oraz rozproszenia punktów na co najmniej 3% widocznego obszaru referencji oraz fotometrycznej weryfikacji wyrównanego obszaru (`minVerificationCorrelation`). Między mapami obowiązuje margines przewagi pierwszego wyniku nad drugim. Powtarzalna siatka, legendy i niewystarczająca tekstura mogą uniemożliwić lokalizację; aplikacja powinna wtedy pozostać w stanie bez automatycznych współrzędnych.

`MapViewMatch` zwraca `status`, `best`, `candidates`, `warning`, `elapsedMs`. Stany JSON: `matched`, `ambiguous`, `noMatch`, `missingReferences`, `timedOut`. Tylko `matched` ma `best`. Po przekroczeniu budżetu nie publikujemy zwycięzcy z częściowo porównanego katalogu.

`MapViewCandidate` zawiera `mapId`, `score`, `inliers`, `tentativeMatches`, `inlierRatio`, `transform`, `projectedMapBounds`, `visibleReferenceBounds`, `coverageRatio`, wymiary query/referencji, jednostkę i kalibrację. `score = sqrt(inlierRatio) × średnia zgodność bitów` jest miarą dopasowania, **nie prawdopodobieństwem poprawnej mapy**. `coverageRatio` szacuje geometryczny udział referencji wewnątrz klatki przez siatkę 16×16; nie mierzy zakrycia przez HUD ani maskę kołowej minimapy.

`SimilarityTransform { scale, rotationDeg, offsetX, offsetY }` mapuje piksele query/klienta do pikseli oryginalnej mapy:

```text
reference = offset + scale × R(rotationDeg) × query
```

`map_point` i `inverse_point` zwracają `Result<Point, String>`. `candidate.reference_point` sprawdza również granice obu obrazów; `calibrated_world_point` stosuje następnie istniejącą kalibrację świata. Punkt poza mapą nie jest ekstrapolowany. `projectedMapBounds` jest osiowym obrysem pełnej referencji rzutowanym do klatki; trzeba przeciąć go z klientem przez `PixelRect::intersect`. Dla obróconej mapy narożniki prostokąta mogą obejmować HUD, dlatego każdy marker należy osobno przekształcić i sprawdzić przez `reference_point`. Progi powierzchni komponentu są przeliczane przez kwadrat skali, jeśli odnoszą się do pikseli referencji.

### Ograniczona konfiguracja

Domyślne wartości według `MapMatchConfig::default()` w `crates/maps/src/registration.rs` (wersja 0.5):

| Pole `MapMatchConfig` | Domyślna wartość | Znaczenie |
|---|---|---|
| `maxImageDimension` | 1024 | Maksymalna oś obrazu roboczego klatki; dopuszczalne 64–1024 |
| `maxFeaturesPerLevel` | 512 | Limit narożników poziomu, 16–1024 |
| `pyramidScales` | `[1, 0.7, 0.5]` | Poziomy klatki; maks. 6, każdy 0.2–1 |
| `rotationsDeg` | `[0]` | Mapa M i minimapa MTC mają północ u góry; inne orientacje trzeba dodać jawnie (maks. 8) |
| `minScale` / `maxScale` | 0.1 / 10 | Zakres skali query → reference, maks. 100 |
| `maxDescriptorHamming` | 34 / 128 bitów | Maksymalna różnica deskryptora, 1–40 |
| `descriptorRatio` | 0.9 | Przewaga względem drugiego, przestrzennie odrębnego punktu; 0.1–0.95 |
| `maxReprojectionError` | 4 px referencji | Dolna granica tolerancji; poziomy zgrubne poszerzają ją do 2 px poziomu, maks. 32 |
| `minInliers` / `minInlierRatio` | 6 / 0.1 | Minimalne wsparcie geometrii; 4–128 / 0.05–1 |
| `ambiguityMargin` | 0.1 | Wymagana różnica score między pierwszą a drugą mapą; 0.01–0.5 |
| `maxCandidates` | 5 | Limit diagnostycznych wyników, do 16 |
| `maxHypotheses` | 512 | Limit deterministycznych hipotez, 16–4096 |
| `maxProcessingMs` | 500 | Budżet pracy; dopuszczalne 1–1500 ms |
| `minVerificationCorrelation` | 0.4 | Korelacja wyrównanej klatki i referencji przy wsparciu 2–3× `minInliers`; +0.25 przy minimalnym konsensusie, −0.15 przy ≥9× `minInliers`; 0.1–0.95 |

Skuteczność tej konfiguracji na 25 teksturach (syntetyczne klatki i prawdziwy zrzut Zone 11): [benchmark](map-matching-benchmark.md).

Orientacja dopasowanej geometrii musi leżeć do 15° od skonfigurowanej orientacji. Dla obrotu 45° należy jawnie dodać go do wyszukiwania; nie obiecujemy ciągłej niezawodności wszystkich rotacji/skal. Znaczny zoom, bardzo mała minimapa, deformacja, fotografia pod kątem lub zakrycie mogą nie dać wystarczających cech. Rozproszenie cech ogranicza dominację mocnego HUD, ale nie gwarantuje znalezienia małej minimapy w dowolnej scenie. Regresja mapy 160 px w klatce 640×480 otoczonej silniejszą szachownicą daje za mało zgodnych cech i poprawnie blokuje wynik. W praktyce może być potrzebne otwarcie większego widoku mapy albo dopasowanie konfiguracji skali.

Budżet jest sprawdzany współpracująco między ograniczonymi etapami/pętlami. Sampler obrazu działa w czasie zależnym od maksymalnego rozmiaru roboczego, nie od pełnego obszaru wejścia; częstotliwość i maksymalna liczba referencji pozostają ograniczone. Systemowy scheduler może wydłużyć rzeczywisty czas. Konsument musi nadal sprawdzić czas źródłowej klatki po dopasowaniu i przed użyciem współrzędnych.

## Heightmap użytkownika

`HeightMap::from_rgba(&image, metadata)` importuje liniową skalę szarości 8 bitów. 0 odpowiada `minHeight`, 255 — `maxHeight`. Kanały R/G/B muszą być równe, alfa 255 oznacza próbkę, alfa 0 — brak danych. Kolorowa mapa i częściowa przezroczystość są odrzucane. PNG jest właściwym formatem zachowującym próbki; zakresy źródła bez rastra nie dostarczają wysokości.

```json
{
  "schemaVersion": 1,
  "mapId": "user-map-a",
  "mapWidth": 800,
  "mapHeight": 600,
  "minHeight": 0,
  "maxHeight": 500,
  "unit": "game_units",
  "source": "Własny pomiar terenu; wersja i data zapisane przez użytkownika",
  "confirmed": true,
  "coordinateAlignmentConfirmed": true
}
```

Metadane wymagają skończonego, rosnącego zakresu bez przepełnienia, jednostki, źródła i dwóch potwierdzeń. `mapWidth/mapHeight` opisują oryginalny obraz tej samej mapy, nie rozmiar klatki klienta. Heightmap może mieć inną rozdzielczość, ale musi przedstawiać ten sam pełny kadr, osie i orientację. Maksymalny raster to 4M próbek, minimum 2×2. Nie stosujemy automatycznego dopasowania heightmap do kolorowej mapy.

`HeightMap::from_normalized(metadata, width, height, &[Option<f64>])` pozwala przekazać siatkę liczbową 0–1; `None` oznacza brak danych. Oba konstruktory zachowują próbki wyłącznie w RAM. Metadane można serializować, a przy ponownym starcie wczytać ten sam wskazany przez użytkownika plik.

`sample_height(point, map_id, unit)` interpoluje biliniowo w pikselach referencji. Nie przyjmuje współrzędnych klienta ani świata bez wcześniejszej transformacji. Zakres punktu to `[0, mapWidth−1] × [0, mapHeight−1]`; nic nie jest ekstrapolowane. Brak próbki o niezerowej wadze, inne ID mapy, niezgodne jednostki lub brak potwierdzeń dają błąd. `height_delta(own, target, map_id, unit)` zwraca wysokość celu minus wysokość stanowiska. Kwantyzacja 8-bitowa ma krok `(maxHeight−minHeight)/255`; potwierdzenie importu nie jest walidacją wysokości w aktualnym MTC.

## Źródła rastrów

Przypięty [helper images.ts](https://github.com/grand-hawk/artillery-calculator/blob/39a7af7689524e57ca2326f3b998cf123df95426/packages/mtc-artillery/src/utils/images.ts) tworzy adresy map i heightmap na serwerze `maps.artillery-calculator.com`. Licencja rastrów nie jest potwierdzona. Pliki trafiają wyłącznie do lokalnego cache użytkownika i nie są dołączane do wydań. Szczegóły: [zasoby map](community-map-assets.md), [pochodzenie danych](community-sources.md).

## Weryfikacja

```powershell
cargo fmt -p mtc-maps --check
cargo test -p mtc-maps -- --test-threads=1
cargo clippy -p mtc-maps --all-targets -- -D warnings
```

Testy deterministyczne obejmują dokładne nazwy i konflikty aliasów, rejestr referencji, mapę wewnątrz większej klatki, zoom, obrót 90°, skonfigurowany 45°, częściowy kadr i transformację do świata, niewłaściwą mapę, dwa podobne ID, powtarzalną teksturę i wyczerpany budżet. Regresja klatki 982×653 z kaflami 25×20, mocnym HUD i dwoma kolorowymi markerami rozpoznaje mapę 350×200; podobne kafle innej referencji pozostają odrzucone. Test geometrii korzysta z większego budżetu diagnostycznego dla równoległego runnera debug; aplikacja zachowuje limit 500 ms i może zwrócić `timedOut` przy obciążeniu. Teren obejmuje bilinear sampling, brzegi, brak próbek, brak potwierdzeń, źle zadane zakresy, mapę/jednostki i brak ekstrapolacji. Każdy raster testowy jest syntetyczny. Wynik testów nie potwierdza realnego HUD/minimapy, parametrów terenowych aktualnego MTC ani fizycznych monitorów/DPI.


## Biblioteka publicznych map w 0.4–0.5

Panel zasobów synchronizuje 25 znanych tekstur i opcjonalnie 21 heightmap do lokalnego cache. Loader akceptuje do 32 map i terenów, weryfikuje SHA oraz metadane i unieważnia wcześniejsze pomiary przy zmianie biblioteki. Od 0.5 interfejs wczytuje bibliotekę z `lenient: true`: brakujący lub zmieniony plik jest pomijany z ostrzeżeniem (`automation_assets_status`), zamiast wyłączać wszystkie mapy. Niezmienione wpisy (ten sam rekord i SHA) używają już zdekodowanego rastra. Heightmapa 8-bit zajmuje 2 B na próbkę zamiast 16 B. Kalibracja ułamkowej skali zachowuje precyzję przy zapisie JSON. Wybrana preferowana mapa ogranicza czas wyszukiwania, ale nadal wymaga dowodu geometrycznego. [Pobieranie, źródła i RED8](community-map-assets.md).
