# Benchmark dopasowania widoku mapy (mtc-maps 0.5)

Pomiar skuteczności `match_map_view` na publicznych teksturach map społeczności. Rastry pochodzą z serwera `maps.artillery-calculator.com` (25 map, cache `mtc-assets`). Licencja redystrybucji rastrów jest niepotwierdzona, dlatego nie ma ich w repozytorium. Benchmark czyta je z katalogu tymczasowego poza repo.

## Jak uruchomić

```powershell
# katalog z plikami <id>.png (np. skopiowanymi z cache mtc-assets)
cargo run -p mtc-maps --release --example map_bench -- C:\Users\<user>\AppData\Local\Temp\mtc-map-bench\maps
# inne ziarno, wybrane scenariusze/mapy, zapis CSV
cargo run -p mtc-maps --release --example map_bench -- <katalog> --seed 7 --mode catalog --only full1920,zoom --focus zone_11 --csv wynik.csv
# prawdziwy zrzut z gry: decyzja dla całego katalogu i dla oczekiwanej mapy
cargo run -p mtc-maps --release --example map_bench -- <katalog> --frame tests\fixtures\live-2026-10-15\full-map-zone11-frontlines.png --expect zone_11
```

Zamiast argumentu można ustawić zmienną `MTC_MAP_BENCH_DIR`. Opcje strojenia (`--dim`, `--levels`, `--features`, `--ncc`, `--min-inliers`, `--hamming`, `--ratio`, `--rotations`, `--budget`) nadpisują `MapMatchConfig`.

## Scenariusze

Każda klatka to syntetyczny „zrzut z gry” zbudowany z tekstury, ze znaną transformacją do pikseli referencji:

| Scenariusz | Opis |
|---|---|
| pełna mapa 1920×1080 | mapa 600–993 px na ciemnym, przyciemnionym tle sceny, HUD (timer, kompas, przyciski, licznik) |
| pełna mapa 1366×768 | jak wyżej, mapa 600–706 px |
| kadr zoom 1.5–4× | okno 700–1000 px z wyciętym fragmentem tekstury; obszar poza oknem to tło |
| minimapa 200–300 px | tekstura w rogu klatki ze sceną 3D (niebo, trawa, drzewa) |
| negatyw: mapa spoza katalogu | pełna mapa, ale jej tekstura jest usunięta z katalogu (24 pozostałe) |
| negatyw: brak mapy | sama scena z HUD albo pusty panel z siatką i markerami |

Na każdą klatkę nakładane są: dodatkowa siatka 6–10 komórek (kolumny literowe E F G H J… bez I, wiersze liczbowe, etykiety w komórkach), 8–25 kolorowych markerów i etykiet punktów, zmiana jasności (0.55–1.1), kontrastu (0.7–1.2) i gammy (0.8–1.4), w połowie przypadków rozmycie σ 0.4–1.3 oraz kompresja JPEG q55–90. Tekstury mają już własną siatkę 9×9, a mapy niekwadratowe mają przezroczyste marginesy.

**Kryterium poprawności:** właściwa mapa i maksymalny błąd w 9 punktach widocznej mapy ≤ max(6 px, 2 px klatki) w pikselach referencji. **Niedokładne** to właściwa mapa z błędem do 4× tej tolerancji (kilka pikseli klatki). **Fałszywe** to inna mapa albo większy błąd. W trybie **katalog 25** porównujemy z całą biblioteką. W trybie **preferowana** porównujemy tylko z jedną referencją; w negatywach jest to inna mapa.

## Wyniki przed i po (ziarno 2026, release, budżet 500 ms)

Maszyna: 8 rdzeni, Windows 11. Pomiary wykonywano przy obciążeniu przez równoległe kompilacje innych agentów, więc czasy są zawyżone. Scenariusze są identyczne w obu kolumnach (to samo ziarno i generator).

### Przed (algorytm 0.4)

| Tryb | Scenariusz | N | Poprawne | Fałszywe | Brak | Timeout | Med. ms |
|---|---|---:|---:|---:|---:|---:|---:|
| katalog 25 | pełna mapa 1920×1080 | 50 | 0 (0%) | 0 | 0 | 50 | 502 |
| katalog 25 | pełna mapa 1366×768 | 25 | 0 (0%) | 0 | 0 | 25 | 502 |
| katalog 25 | kadr zoom | 50 | 0 (0%) | 0 | 0 | 50 | 502 |
| katalog 25 | minimapa | 25 | 0 (0%) | 0 | 0 | 25 | 501 |
| katalog 25 | negatyw: spoza katalogu | 25 | — | 0 | 0 | 25 | 501 |
| katalog 25 | negatyw: brak mapy | 10 | — | 0 | 0 | 10 | 501 |
| preferowana | pełna mapa 1920×1080 | 50 | 2 (4%) | 1* | 47 | 0 | 102 |
| preferowana | pełna mapa 1366×768 | 25 | 3 (12%) | 3* | 19 | 0 | 101 |
| preferowana | kadr zoom | 50 | 8 (16%) | 0 | 42 | 0 | 101 |
| preferowana | minimapa | 25 | 0 (0%) | 0 | 25 | 0 | 98 |
| preferowana | negatyw: spoza katalogu | 25 | — | 0 | 25 | 0 | 94 |
| preferowana | negatyw: brak mapy | 10 | — | 0 | 10 | 0 | 79 |

\* Pierwsza wersja harnessu nie miała kolumny „niedokładne”. Były to właściwe mapy z błędem powyżej tolerancji.

Z pełnym katalogiem stary matcher **zawsze** przekraczał budżet: cechy wszystkich referencji liczył od nowa przy każdej klatce. W aplikacji bez wybranej preferowanej mapy oznaczało to stałe `timedOut`.

### Po (algorytm 0.5)

| Tryb | Scenariusz | N | Poprawne | Niedokładne | Fałszywe | Brak | Timeout | Med. błąd [px ref] | Med. ms | p95 ms |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| katalog 25 | pełna mapa 1920×1080 | 50 | 50 (100%) | 0 | 0 | 0 | 0 | 0.9 | 60–67 | 65–74 |
| katalog 25 | pełna mapa 1366×768 | 25 | 25 (100%) | 0 | 0 | 0 | 0 | 1.0 | 58–62 | 62–68 |
| katalog 25 | kadr zoom | 50 | 43 (86%) | 0 | 0 | 7 | 0 | 0.5 | 60–67 | 66–74 |
| katalog 25 | minimapa | 25 | 5 (20%) | 6 | 0 | 14 | 0 | 5.5 | 56–62 | 63–70 |
| katalog 25 | negatyw: spoza katalogu | 25 | — | 0 | 0 | 25 | 0 | — | 59–65 | 63–74 |
| katalog 25 | negatyw: brak mapy | 10 | — | 0 | 0 | 10 | 0 | — | 50–53 | 55–59 |
| preferowana | pełna mapa 1920×1080 | 50 | 50 (100%) | 0 | 0 | 0 | 0 | 0.9 | 44 | 52 |
| preferowana | pełna mapa 1366×768 | 25 | 25 (100%) | 0 | 0 | 0 | 0 | 1.0 | 40 | 50 |
| preferowana | kadr zoom | 50 | 43 (86%) | 0 | 0 | 7 | 0 | 0.5 | 43 | 49 |
| preferowana | minimapa | 25 | 5 (20%) | 6 | 0 | 14 | 0 | 5.5 | 40 | 47 |
| preferowana | negatyw: spoza katalogu | 25 | — | 0 | 0 | 25 | 0 | — | 42 | 51 |
| preferowana | negatyw: brak mapy | 10 | — | 0 | 0 | 10 | 0 | — | 36 | 39 |

Zakresy czasu pochodzą z dwóch przebiegów tego samego ziarna przy różnym obciążeniu maszyny.

### Inne ziarna (katalog 25, release)

| Ziarno | Pełna 1920 | Pełna 1366 | Zoom | Minimapa (popr. + niedokł.) | Fałszywe (wszystkie scenariusze) |
|---|---:|---:|---:|---:|---:|
| 7 | 50/50 | 25/25 | 42/50 | 10 + 1 / 25 | 0 / 185 |
| 99 | 49/50 | 25/25 | 43/50 | 7 + 3 / 25 | 0 / 185 |

Dla 12 map z aktualnej rotacji (Arctic Airbase, Dustbowl II, Fulvia Gap, Muddy Fields, Normandy, Rohkshort, Rohkstov, Roinburg, Snowy Fields, Sokolovka, Villers-Sommeil, Zone 11) sumarycznie dla ziaren 2026, 7 i 99 (katalog 25):

- pełna mapa: 108/108;
- zoom: 60/72;
- minimapa: 8/36 poprawnych i 4 niedokładne;
- negatywy: 0/36 fałszywych.

**Stres najgorszego przypadku:** Dustbowl, Dustbowl II i Dustbowl II Rework, każda usunięta z katalogu, 20 ziaren, czyli 60 klatek. Wynik: 0 fałszywych dopasowań. Przed poszerzeniem obszaru weryfikacji było 3/45.

### Prawdziwy zrzut z gry

`tests/fixtures/live-2026-10-15/full-map-zone11-frontlines.png` to pełna mapa M, Zone 11, 660×720, z liniami frontu, ikonami pojazdów i etykietami siatki. Wynik dla katalogu 25: `matched zone_11`, 30 inlierów, korelacja 0.49, jedyny kandydat, 53 ms w release i 560 ms w debug. Transformacja umieszcza narożnik tekstury w (32, 92) px przy szerokości 575 px. Ramka mapy na zrzucie zaczyna się w ≈(37, 92) i ma ≈568 px, czyli błąd jest rzędu kilku pikseli klatki. To jedna klatka: daje dowód działania, ale nie statystykę.

### Profil debug

Przy 6 mapach (`--quick`) pełna klatka 1920×1080 zajmuje ≈420 ms. Prawdziwy zrzut 660×720 przy 25 mapach zajmuje 560 ms. Rejestracja jednej tekstury trwa w debug mediana ≈0,86 s, max 1,6 s, a w release ≈80 ms (max 140 ms). Kompilacja debug aplikacji z 25 mapami może więc zwracać `timedOut` przy budżecie 500 ms. Testy jednostkowe nie zależą od zegara, a cały zestaw `cargo test -p mtc-maps` trwa ≈2 s.

## Co się zmieniło w algorytmie

1. **Cechy referencji liczone raz przy `MapReference::new`.** Gęsta piramida obejmuje maks. 1024 px, kolejne poziomy ×0,8 aż do ≈96 px, z limitem cech zależnym od powierzchni poziomu. Klony współdzielą indeks przez `Arc`. Wcześniej każde wywołanie liczyło cechy wszystkich referencji, co przy 25 mapach dawało 100% `timedOut`.
2. **Uśrednianie powierzchniowe zamiast próbkowania punktowego.** Poprawny antyaliasing przy redukcji 1920→1024 i 4096→1024 oraz spójna konwencja środków pikseli `(x+0,5)·r−0,5`.
3. **Detektor i deskryptor:**
   - wygładzanie binomialne przed Shi–Tomasi i BRIEF;
   - NMS 3×3 i niski próg absolutny, więc przyciemnione kadry nie tracą narożników;
   - siatka zajętości O(1) zamiast porównań O(n²);
   - rozkład przestrzenny w kubełkach zależnych od limitu.
4. **Przezroczyste marginesy tekstur są maskowane.** Bez cech, których wsparcie BRIEF wchodzi w alfę < 0,5, i bez próbek weryfikacji z alfą < 0,9.
5. **Dopasowanie:**
   - test stosunku jednym przebiegiem z drugim *przestrzennie odrębnym* punktem;
   - sprzętowy POPCNT wykrywany w czasie wykonania;
   - referencje dopasowywane równolegle (`std::thread::scope`) z wynikiem w kolejności katalogu, więc deterministycznym.
6. **RANSAC:**
   - para poziomów piramidy wyznacza oczekiwaną skalę; hipoteza i inliery muszą się z nią zgadzać (±0,3 w log-skali);
   - tolerancja reprojekcji zależy od poziomu;
   - wyczerpujące wyliczenie przy małej liczbie par;
   - zdegenerowana próbka pomija hipotezę, a nie całą mapę.
7. **Weryfikacja fotometryczna wyrównanego obszaru:**
   - odporna korelacja (odrzucenie skrajnych próbek osobno w każdym obrazie) liczona globalnie i w kafelkach 4×4; wynik to minimum obu;
   - pomijanie płaskich fragmentów klatki (panele UI) i linii siatki 9×9 wspólnej dla wszystkich tekstur;
   - obszar wokół inlierów obejmuje 20–50% widocznej mapy, tym więcej, im słabszy konsensus;
   - wymagana korelacja rośnie dla konsensusu bliskiego minimum i skupionych inlierów, a maleje dla przytłaczającego konsensusu.
8. **Budżet czasu:**
   - wewnętrzna ścieżka bez zegara dla testów geometrii;
   - publiczne `match_map_view` zawsze używa terminu;
   - przekroczenie terminu w dowolnym wątku blokuje wynik (`timedOut`).

## Ograniczenia

- **Minimapa 200–300 px:** 20–40% poprawnych, część z błędem 2–4 px klatki (8–18 px referencji). Na zdjęciach z gry minimapa to ciemny schemat siatki z ikonami, a nie tekstura terenu, więc dopasowanie tekstury nie jest tam właściwą metodą. Do minimapy lepsza jest detekcja linii siatki 9×9 i etykiet (skala i przesunięcie), co jest już realizowane osobno.
- **Wersje tej samej mapy** (Dustbowl / Dustbowl II) mają wspólny kontur, siatkę i główne drogi. Przy obu w katalogu właściwa wygrywa marginesem; przy brakującej wersji stres dał 0/60 fałszywych, ale to nie jest gwarancja. Interfejs powinien pokazywać nazwę rozpoznanej mapy.
- **Zoom ≥3×** w małym oknie przy teksturach o niskim kontraście (Waterloo, Muddy Fields, The Map) bywa odrzucany (`noMatch`). To jest stan bezpieczny.
- Domyślne wyszukiwanie dotyczy wyłącznie orientacji północ u góry. Obroty trzeba jawnie dodać w `rotationsDeg`.
- Klatki benchmarku są syntetyczne. Wynik nie potwierdza wszystkich wariantów HUD, rozdzielczości, skalowania DPI ani aktualności tekstur względem gry.
