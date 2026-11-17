# Analiza obrazu i śledzenie widocznych markerów

Moduł `mtc-vision` przetwarza lokalnie jedną klatkę przechwyconego okna. W jednym przebiegu odczytuje wiele obszarów OCR oraz szuka widocznych markerów na jawnie skalibrowanym obszarze mapy. Nie korzysta z sieci. Klatki zapisuje wyłącznie `RecordingWriter`, uruchamiany przez aplikację po jawnej zgodzie użytkownika (sekcja „Nagrania testowe”).

## Dostępne funkcje

- `analyze_frame(image, timestamp_ms, now_ms, config)` zwraca `FrameAnalysis`: wyniki każdego ROI, obserwacje liczbowe i tekstowe, kandydatów markerów oraz diagnostykę. Niezależne ROI mogą mieć niezależne błędy.
- `stabilize_frame(current, previous)` zwiększa heurystyczną pewność zgodnych odczytów z tego samego ROI. Zmiana okna lub konfiguracji wymaga wyczyszczenia poprzedniej analizy.
- `invalidate_frame(analysis, status, reason)` usuwa także obserwacje z zagnieżdżonych wyników ROI. Konsument wywołuje je, jeżeli klatka przeterminowała się podczas OCR lub zmieniło się źródło.
- `calibrate_view_signature(image, map_roi)` tworzy mały deskryptor jawnie wybranego widoku mapy. `match_view_signature(...)` sprawdza podobieństwo bieżącego widoku do tej referencji.
- `detect_markers(...)` zwraca wszystkie widoczne komponenty spełniające progi podane przez użytkownika. Nie wybiera automatycznie jednego z kilku kandydatów tej samej roli.
- `estimate_motion(samples, calibration, now_ms, tracker_config)` estymuje prędkość w skalibrowanych jednostkach świata, w osiach wschód/północ. Wymaga co najmniej dwóch świeżych pozycji tego samego ręcznie wybranego markera.
- `predict_position(estimate, seconds)` stosuje ograniczony w czasie model stałej prędkości. Nie mierzy wysokości ani przyspieszenia.

Dotychczasowe `ocr`, `parse_numeric`, `Observation`, `Roi`, `is_fresh` i `stabilize` pozostają dostępne. `analyze_frame_with_ocr` pozwala podstawić deterministyczny odczyt w testach; aplikacja używa systemowego OCR Windows.

## Konfiguracja wielu obszarów

Poniższy przykład pokazuje format JSON, a jego wartości ROI są przykładowe. Nie są uniwersalną konfiguracją HUD MTC. Należy wyznaczyć je na aktualnym podglądzie okna. `referenceWidth` i `referenceHeight` oznaczają rozdzielczość klatki, przy której ustawiono ROI. Skalowanie zachowujące proporcje działa; zmiana proporcji ponad `aspectTolerance` blokuje pomiary do ponownej kalibracji.

```json
{
  "schemaVersion": 1,
  "referenceWidth": 1920,
  "referenceHeight": 1080,
  "aspectTolerance": 0.03,
  "maxAgeMs": 3000,
  "ocrRegions": [
    {
      "id": "match-timer",
      "kind": "timer",
      "roi": {"x": 0.43, "y": 0.01, "width": 0.14, "height": 0.09},
      "upscale": 2
    },
    {
      "id": "explicit-vehicle-label",
      "kind": "vehicleName",
      "roi": {"x": 0.01, "y": 0.85, "width": 0.2, "height": 0.08},
      "upscale": 2
    }
  ],
  "markerConfig": null
}
```

Obsługiwane `kind`: `timer`, `heading`, `altitude`, `speed`, `vehicleName`, `weaponName`, `mapName`, `text`. Nazwa pojazdu lub broni jest wyłącznie odczytanym napisem. Moduł nie rozpoznaje modelu po wyglądzie i nie przypisuje automatycznie parametrów uzbrojenia na podstawie niepewnego tekstu. Jednostki wysokości i prędkości pozostają niepotwierdzone, dopóki nie zostaną sprawdzone w grze.

OCR obejmujący kilka liczb kompasu zwraca `ambiguous`, zamiast wybierać dowolną liczbę. Liczba sklejona z literami (`13B`, `N138`) nie jest odczytem; kurs dopuszcza tylko znak `°`, a prędkość i wysokość — jednostkę (`m`, `ft`, `km/h`, `kt`, `m/s` itp.). Czas `MM:SS` sąsiadujący z kolejnym dwukropkiem lub cyfrą (np. `1:05:30`) jest odrzucany. W tokenie z co najmniej dwiema cyframi poprawiana jest najwyżej jedna typowa pomyłka OCR: `O`/`o` → `0`, `l`/`I`/`|` → `1` (np. `l38` → 138). Słowa bez cyfr nie są zmieniane. Brak tekstu daje `empty`; błąd OCR daje `error`. Stare i przyszłe klatki są blokowane przed kosztownym OCR. Czas pomiaru jest czasem klatki. Konsument musi ponownie sprawdzić świeżość po zakończeniu analizy oraz przed obliczeniami.

## Przygotowanie obrazu do OCR

`read_roi_text(image, roi, upscale, kind, recognize)` wykonuje do trzech deterministycznych prób dla jednego ROI:

1. zwykłe powiększenie Catmull-Rom ×`upscale` (dotychczasowe zachowanie),
2. wariant „jasny tekst” ×`upscale`: minimum kanałów RGB (biały tekst pozostaje jasny, nasycone niebo/tło ciemnieje), rozciągnięcie kontrastu między 2. a 98. percentylem, inwersja do ciemnych znaków na jasnym tle i margines w kolorze mediany krawędzi,
3. ten sam wariant przy powiększeniu o 1 większym.

Kolejna próba jest wykonywana tylko wtedy, gdy poprzednia nie dała dokładnie jednej poprawnej wartości (pola liczbowe) albo dała pusty tekst (pola tekstowe). Wygrywa pierwszy zaakceptowany odczyt. Gdy żaden nie przejdzie, wynik pozostaje `ambiguous`/`empty` z tekstem pierwszej próby — wariant nie tworzy wartości. Błąd systemowego OCR kończy próby. `analyze_frame`, `analyze_frame_with_ocr` (wstrzykiwany OCR może więc być wywołany do 3 razy na ROI) oraz komenda `analyse_file` korzystają z tej samej ścieżki; `analyse_file` nie rozciąga już szerokich ROI niejednorodnie (wcześniej każda oś miała osobny limit 2500 px).

`Roi::pixel_rect` jest wspólną regułą przeliczenia ROI na piksele (lewa/górna krawędź w dół, rozmiar zaokrąglony, przycięcie do obrazu). Używają jej wycinek OCR, detektor markerów, kalibracja kliknięcia i przeliczenie punktu na mapę referencyjną. `preview_thumbnail` tworzy miniaturę podglądu bez kopiowania całej klatki.

## Markery i kalibracja mapy

Włączanie CV wymaga jawnej zgody na automatyczny odczyt, wskazania ROI całej widocznej mapy, potwierdzenia zgodności z mapą referencyjną i zapamiętania deskryptora tego widoku. `mapWidth` i `mapHeight` to rozmiary tego samego obrazu mapy, na którym ustawiono skalę i punkty referencyjne. Przycięta minimapa, inny poziom zoomu lub mapa z dodatkowymi marginesami wymaga osobnej kalibracji. Sama zgodność proporcji nie potwierdza treści mapy.

Przykładowa wartość `markerConfig`:

```json
{
  "enabled": true,
  "mapRoi": {"x": 0.25, "y": 0.2, "width": 0.5, "height": 0.666667},
  "mapWidth": 800,
  "mapHeight": 600,
  "coordinateCalibrationConfirmed": true,
  "viewSignature": null,
  "aspectTolerance": 0.03,
  "signatures": [
    {
      "id": "own-user-selection",
      "role": "own",
      "rgb": [0, 240, 0],
      "tolerance": 12,
      "minComponentPixels": 20,
      "maxComponentPixels": 120,
      "minAspectRatio": 0.5,
      "maxAspectRatio": 2.0,
      "minFillRatio": 0.2
    }
  ]
}
```

`viewSignature: null` w przykładzie celowo blokuje pozycje, dopóki użytkownik nie zapamięta rzeczywistego widoku mapy. Przycisk kalibracji wywołuje `calibrate_view_signature` na wybranej klatce i ROI. Wynik ma `schemaVersion: 1`, `gridSize: 16`, `graySamples` (256 wartości `u8`), `textureRms`, `aspectRatio` i `threshold: 0.85`. Nie należy ręcznie wpisywać przykładowych próbek. Brak pola w starszych ustawieniach jest migrowany do braku deskryptora.

Deskryptor zawiera medianę jasności z 49 próbek każdej komórki siatki 16 × 16. Porównanie wykorzystuje korelację jasności i gradientów; usuwa globalną zmianę jasności/skali kontrastu i niewielką część miejscowych różnic powodowanych małymi ikonami. Próg wynosi co najmniej `0.85`. Korelacja jest miarą podobieństwa, a nie prawdopodobieństwem poprawnej mapy. Zwykły widok pojazdu z przypadkowo pasującymi kolorami powinien nie przejść tej bramki. Obrazy jednolite, z bardzo słabą teksturą albo samym płaskim gradientem jasności nie pozwalają na wiarygodny deskryptor — kalibracja zwraca błąd i pozostaje tryb Manual. Minimalny ROI wynosi 48 × 48 px.

To dopasowanie konkretnego widoku wybranego przez użytkownika. Nie rozpoznaje semantycznie nazwy mapy, nie dowodzi zgodności współrzędnych i nie zastępuje potwierdzenia jej osi, skali oraz pełnego kadru. Nietypowe podobne obrazy mogą przejść prosty deskryptor, a duże napisy, zmiana zoomu czy szczegółów HUD mogą odrzucić prawidłowy widok. W razie zmiany widoku należy ponownie skalibrować referencję. Zapisywany jest wyłącznie niewielki deskryptor, bez pełnej klatki.

Kolor w przykładzie jest syntetyczny. Użytkownik wybiera rzeczywisty kolor i przypisuje rolę `own`, `target`, `reference` albo `unassigned`. Kolor sam w sobie nie określa drużyny, przeciwnika ani własnej pozycji. Legenda i znaczniki mogą mieć ten sam kolor, dlatego wiele kandydatów wymaga wskazania właściwego przez użytkownika. Nakładające się sygnatury koloru, powierzchni i proporcji są odrzucane, aby jedna ikona nie otrzymała sprzecznych ról. Obserwacja nie zawiera identyfikacji ukrytego obiektu.

Detektor używa ośmiosąsiednich spójnych komponentów RGB. Odrzuca komponenty o niewłaściwej powierzchni, proporcjach lub wypełnieniu. `tolerance` jest maksymalną różnicą każdego kanału RGB; powierzchnia jest przeliczana do pikseli referencyjnej mapy, więc podwojenie rozdzielczości nie wymaga podwojenia progów. Próg powierzchni i kształtu należy dobrać na podglądzie; nie jest modelem specyficznym dla MTC. Przycięty marker przy krawędzi otrzymuje niską pewność. Ponad 512 kandydatów blokuje wynik i wymaga zawężenia progów.

Detektor wycina mapę raz i na tym samym wycinku sprawdza deskryptor widoku (wcześniej mapa była kopiowana dwukrotnie). Bufor odwiedzin i kolejka BFS są współdzielone przez wszystkie sygnatury klatki.

Wynik markera zawiera `mapPosition` w pikselach mapy referencyjnej, `framePositionNormalized` i `boundingBox` do podglądu, `pixelCount`, jawnie przypisaną rolę, ID sygnatury, czas klatki i heurystyczną pewność. Maksymalnie osiem sygnatur i szesnaście ROI ogranicza koszt analizy. Silne zmiany oświetlenia, skalowanie ikon i nakładające się komponenty mogą wymagać ponownego ustawienia progów.

## Ruch i wyprzedzenie

`PositionSample` ma `sourceId`, `mapPosition`, `timestampMs`, `confidence`. `sourceId` oznacza konkretny wybrany marker; nadanie jednego ID wszystkim czerwonym ikonom nie zapewnia śledzenia tożsamości. Historię należy wyczyścić po zmianie wyboru, okna, ROI, mapy lub jej kalibracji. W przypadku kilku pasujących markerów aplikacja wstrzymuje automatyczne użycie pozycji.

`TrackerConfig` zawiera rozmiary mapy, maksymalny wiek danych, minimalny odstęp obserwacji, limit prędkości, minimum pewności, liczbę ostatnich próbek i maksymalny horyzont predykcji. Domyślny limit 500 jednostek/s służy odrzucaniu skoków; nie jest zweryfikowaną prędkością pojazdów MTC. Użytkownik powinien dobrać go do jednostek swojej mapy.

Stany wyniku: `collecting` — za mało danych, `ready` — dostępna prędkość, `stale` — brak świeżej pozycji, `rejected` — skok, błędny czas, niska pewność, zmiana tożsamości, punkt poza mapą lub ruch źle pasujący do stałej prędkości. W każdym stanie poza `ready` prędkość pozostaje pusta. Stara historia nie jest używana w dopasowaniu. Wysokość celu wymaga osobnego pomiaru; model płaszczyzny mapy nie wystarcza do pełnego rozwiązania przeciwlotniczego.

## Panel stanowiska broni MTC i pełna mapa (fixtures 2026-10-15)

`read_weapon_panel(obraz, czas, znane_nazwy)` lokalizuje panel geometrycznie: prostokątne ciemne wyświetlacze LED w prawych 32% klatki (nazwa broni/amunicji, pole RANGE, wyświetlacz ELEVATION/AZIMUTH), wyrównane w pionie; rozmiary skalują się z wysokością klatki (`wysokość / 1080`). Nazwy czyta OCR Windows (maska żółtego tekstu, skale 2–4, głosowanie); nazwa z katalogu albo z `OBSERVED_PANEL_NAMES` wygrywa, gdy pasuje dokładnie lub po złożeniu pomyłek fontu (`S-SKI` → `S-5K1`, `PG-g` → `PG-9`) w co najmniej dwóch próbach. Kąty dekoduje rozpoznawanie wzorcowe (`mtc_led.rs`) z gramatyką `-?d{1,3}.dd°`; OCR Windows gubił kropki i mylił `9`/`g`. Wynik spoza gramatyki lub ze zbyt dużym kosztem dopasowania jest pomijany. RANGE `0000` oznacza brak pomiaru (`rangeMeters: null`). Stanowiska bez wyświetlacza kątów (np. SPG-9) dają `partial`. `stabilize_weapon_panel` podnosi pewność do 0,9 dla dwóch świeżych (≤ 2 s) identycznych odczytów.

Wzorce (`src/mtc_led_model.json`) wytrenowano przykładem `train_led` na 43 klatkach 1080p opisanych w `tests/fixtures/live-2026-10-15/led-labels.json`; klatki fixtures wyłączono z treningu. Dwukrotna walidacja krzyżowa na połowie danych: 77/88 wartości; model końcowy: 14/14 wartości na klatkach fixtures i 102/102 na wszystkich 51 opisanych klatkach; brak wartości na 61 klatkach bez panelu. Inne rozdzielczości, skalowanie UI i fonty wymagają nowych próbek.

`read_mtc_full_map(obraz, czas, katalog)` wykrywa siatkę 9 × 9 cienkich jasnych linii (profil grzbietów kanału min(RGB), dopasowanie 10 równoodległych linii, dopasowanie najmniejszych kwadratów), czyta podpis „Grid size: N meters” pod lewym dolnym rogiem (6 prób, wymagane 2 zgodne) i identyfikuje mapę po rozmiarze pola z katalogu (±1,5 m; kilka map → `ambiguous` z listą kandydatów). Zwraca prostokąt siatki w pikselach klienta, wymiary mapy, kalibrację (świat: metry na wschód od zachodniej krawędzi i na północ od południowej) oraz sektor `A1`–`J9` (kolumny bez I, wiersze od góry). Na fixture Zone 11 linie leżą w x 32→609, y 92→668 kadru (pole ~64,1 px). Minimapa ma tę samą siatkę, ale bez podpisu — status `unidentified`.

Interfejs: zakładka **Celowanie (gra na żywo)** odczytuje panel sekwencyjnie co ok. 500 ms, ukrywa odczyty starsze niż 2 s i liczy korektę `wymagane − bieżące` (logika i testy: `apps/desktop/src/aim-correction.ts`). Opis dla użytkownika: [README — Gra na żywo](../README.md#gra-na-żywo-mtc).

**Skróty mapy (aplikacja):** `configure_map_point_hotkeys` rejestruje globalne Ctrl+Shift+T (cel) i Ctrl+Shift+G (własna pozycja). Naciśnięcie odczytuje tylko pozycję kursora (`cursor_client_position`, piksele fizyczne klienta), pobiera klatkę, wykonuje `read_mtc_full_map` i emituje `mtc-map-point-picked` z ułamkiem, sektorem i metrami świata albo z błędem (mapa M zamknięta, kursor poza siatką/oknem). Nic nie jest klikane ani wysyłane do gry.

**Ekran wyboru broni:** `read_loadout_specs` czyta OCR panel „Specifications” (Damage, Type, Muzzle Velocity m/s, Penetration mm, Projectile Mass kg) i slot z tytułu „Choose Weapon - …”. Wartości bez nagłówka panelu lub bez jednostki są odrzucane. Panel nie podaje nazwy broni (`weaponName: null`). Fixtures: `loadout-specs-primary.png` (825 m/s), `loadout-tertiary.png` (350 m/s).

**Własna pozycja (beta):** `read_own_position(obraz, czas, mapa)` szuka białej strzałki gracza w siatce minimapy lub mapy M: jasne, nienasycone komponenty (próg adaptacyjny 0,75 × 99,8 percentyla, więc działa też przy przyciemnionej minimapie) porównane z obróconym szablonem szewronu (IoU ≥ 0,72; białe ikony pojazdów osiągają ≤ 0,63). Kierunek strzałki (kurs postaci) musi zgadzać się z kompasem ±20°, inaczej status `unconfirmed` (kamera obrócona względem postaci). Kilka strzałek → `ambiguous`; strzałka zasłonięta → `notFound`. Siatka z niewidoczną krawędzią (np. zasłoniętą paskiem zadań) jest odrzucana, bo przesunięta o pole siatka pasowałaby równie dobrze. Współrzędne świata wymagają mapy wybranej w aplikacji (minimapa nie ma podpisu „Grid size”). `stabilize_own_position` daje 0,9 po dwóch zgodnych klatkach z potwierdzeniem kompasu; UI ustawia punkt działa od 0,85. Na 28 klatkach oznaczonych wzrokowo (Rohkstov, piechota): 26 trafień (20 `ready`, 6 `unconfirmed`), 0 fałszywych pozycji, 2 braki (zasłonięta krawędź siatki, przyciemniona minimapa). Na pozostałych klatkach sesji brak wykryć poza rzeczywistą strzałką (sprawdzone wzrokowo w klatkach z pojazdami). Pełna mapa M z piechotą nie była dostępna w nagraniach — niezweryfikowana.

## Nagrania testowe i analiza offline

`RecordingWriter::create(katalog, RecordingOptions, now_ms)` odmawia bez `consent: true`, tworzy w wybranym katalogu nowy podkatalog `mtc-recording-<ms>` (nigdy nie używa istniejącego) i zapisuje `session.json`. `offer(obraz, czas_źródła, sekwencja, qpc, now)` zapisuje `frame-000001.png`/`.jpg` (RGB, bez alfy) tylko gdy klatka jest świeża (≤ 2 s, nie z przyszłości), nie jest duplikatem sekwencji i mieści się w limicie FPS. Limity: 0,1–5 FPS, 1–10 000 klatek, 1 MB–8 GB, 1 s–60 min; przekroczenie kończy nagranie bez zapisu kolejnej klatki. Manifest jest odświeżany co 10 klatek i przy `finish`, więc przerwane nagranie pozostaje czytelne (`completed: false`).

`session.json` (`schemaVersion: 1`, `kind: mtc-tactical-assistant-recording`) zawiera: zgodę (`explicit`, czas), źródło (HWND, tytuł, `wgc`/`gdi`), format, limity, `analysisConfig` z ROI w chwili startu (albo `null`, gdy konfiguracja była niepoprawna), listę klatek (`index`, `file`, `timestampMs` źródła, `sourceTimestamp100ns`, `sequence`, wymiary, bajty), sumę bajtów, czas zakończenia i powód zatrzymania.

`load_recording` sprawdza wersję, zgodę, limity i nazwy plików (`frame-NNNNNN.ext`, bez ścieżek — manifest nie może wskazać pliku spoza nagrania). `analyse_recording(ścieżka, config, start, count)` analizuje do 500 klatek na wywołanie przez `analyze_frame` w czasie zapisanej klatki (`now = timestamp`), stabilizuje kolejne klatki jak na żywo i zwraca wyniki klatek oraz podsumowanie ROI (odczyty, stabilne ≥ 85%, niejednoznaczne, puste, błędy). Brak klatki przerywa ciągłość stabilizacji. Pojedynczą klatkę można też otworzyć istniejącym `analyse_file`/`analyse_batch_file`.

## Weryfikacja

Uruchomiono `cargo test -p mtc-vision` w Windows (październik 2026): 82 testy zakończone powodzeniem, w tym 4 testy rzeczywistego OCR na fotografiach. Regresje obejmują fallback jasnego tekstu, odrzucanie `13B`/`1:05:30`, korekty `l38`/`O90`, krawędzie `pixel_rect`, miniatury oraz zapis, limity, odporność na podmieniony manifest i odtwarzanie nagrań.

**OCR fotografii.** Czas `27:42` (BM-21) i `29:33` (śmigłowiec) jest odczytywany przy każdym powiększeniu 1–4. Bez fallbacku zwykłe powiększenie zawodziło dla `29:33` przy ×3 (pusty tekst) i ×4 (`29`). Centralna liczba kompasu `138` na BM-21 (biały tekst na jasnoniebieskim niebie, pod napisem CONQUEST) jest odczytywana z ręcznego ROI w trzech wariantach kadru przy ×2, ×3 i ×4 (9/9); bez fallbacku ciasny kadr dawał pusty wynik przy ×3 i ×4. Fragment nieba obok kompasu nie daje kursu. Pełnoklatkowy OCR widzi `138`, ale nie odczytuje bladych oznaczeń skali (`090`, `180`, `225`), więc automatyczne discovery nadal nie proponuje kursu — wymaga ręcznego ROI. Binaryzacja Otsu okazała się niestabilna i nie jest używana.

Wcześniejszy przebieg: 31 testów. Testy obejmują rzeczywisty systemowy OCR czasu meczu na obu dostarczonych fotografiach, analizę kilku ROI na jednej klatce, niezależne błędy odczytu, jednoznaczność, przeterminowanie wszystkich warstw obserwacji, zmianę geometrii, komponenty syntetycznej teksturowanej mapy, skalowanie 2×, odrzucanie niepotwierdzonej kalibracji, zmianę widoku i śledzenie ruchu. Deskryptor ma regresje zmian jasności, lokalnych ikon, braku tekstury, obrotu i niewłaściwych proporcji. Clippy z `-D warnings` i sprawdzenie formatowania również przeszły.

Fotografie pokazują czas `27:42` i `29:33`. Nie potwierdzają koordynatów mapy, parametrów broni ani jawnej nazwy pojazdu. Test nazw używa jawnie syntetycznego odczytu kontrolnego. Detektor markerów i tracker mają testy syntetyczne; trafność na aktualnej mapie i HUD MTC wymaga sprawdzenia podczas rzeczywistej rozgrywki.

## Źródła API

Systemowy odczyt korzysta z [`Windows.Media.Ocr.OcrEngine`](https://learn.microsoft.com/en-us/uwp/api/windows.media.ocr.ocrengine). [`OcrResult`](https://learn.microsoft.com/en-us/uwp/api/windows.media.ocr.ocrresult) udostępnia tekst i wiersze, bez statystycznego wskaźnika pewności. Podawane w aplikacji wartości pewności OCR i CV są heurystykami, a nie prawdopodobieństwem poprawnej identyfikacji. Implementacja komponentów i dopasowania ruchu jest własna; nie skopiowano kodu z zewnętrznych repozytoriów.
