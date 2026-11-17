# MTC Tactical Assistant 0.5.1

Aplikacja Windows dla Multicrew Tank Combat: przezroczysta nakładka, analiza obrazu gry, kalkulator mapy i artylerii oraz lokalne profile uzbrojenia. Interfejs po polsku; Tauri 2, React i TypeScript, obliczenia oraz przechwytywanie w Rust. Symulację i dostarczone fotografie można analizować bez uruchamiania Roblox.

## Uruchomienie i pierwszy wynik

Uruchom `release/MTC-Tactical-Assistant-0.5.1.exe` lub instalator NSIS z katalogu `release`. Wymagania: Windows 10 2004 lub nowszy / Windows 11 x64, WebView2 Runtime, sterownik D3D11 i systemowy język OCR, np. polski lub angielski. Instalator jest przeznaczony dla bieżącego użytkownika. Wydanie pozostaje niepodpisane cyfrowo.

W rozpakowanym pakiecie ZIP oba pliki `.exe` leżą obok README. Katalog `verification` zawiera raporty i obrazy wyłącznie z testów interfejsu aplikacji. Instrukcje budowania poniżej dotyczą źródeł projektu.

1. Kliknij **Symulacja**, aby sprawdzić dystans, azymut, elewację i czas lotu. Baner oznacza dane syntetyczne, bez odniesienia do parametrów MTC.
2. W **Mapa i cel → Mapa z katalogu** jednorazowo zaakceptuj źródło społeczności i kliknij mapę (wyszukiwarka, najpierw mapy w rotacji). Aplikacja wczytuje teksturę i heightmapę z biblioteki lub cache albo pobiera je. Skala wynika z wymiaru mapy, północ jest u góry, a jednostką są metry gry.
3. Lewy klik stawia działo, prawy cel; punkty można przeciągać, kółko myszy przybliża. Od razu widać sektory, dystans, azymut, różnicę wysokości z heightmapy i — dla profilu w metrach gry — elewację oraz czas lotu. Własny obraz z kalibracją A–B jest w trybie **Własny obraz (zaawansowane)**.
4. Wyszukaj pojazd/amunicję. W **Profile i balistyka** zastosuj własną kalibrację albo, dla konkretnego profilu społecznościowego, zaznacz **Używaj tych eksperymentalnych danych dla wybranego pocisku**. Dla danych społeczności mapa musi używać **Metrów gry**. Potwierdzenie pozostawia status eksperymentalny; profile kierowane nadal mają wyłączony solwer.
5. Kliknij **Pokaż nakładkę**. Gdy przepuszczanie kliknięć jest wyłączone, można przeciągać jej nagłówek.

Katalog obejmuje **153 profile pocisków dla 51 nazw uzbrojenia**, **7 szablonów lokalnej kalibracji** i wymiary **25 map**. Wymiar mapy społecznościowej można zastosować dopiero po wyborze nazwy i potwierdzeniu, że obraz obejmuje całą kwadratową mapę. Panel zasobów udostępnia lokalne pobranie 25 tekstur i 21 map wysokości z tego samego źródła. Szczegóły pochodzenia: [dane społeczności](community-sources.md).

Skróty globalne: **Ctrl+Shift+O** — pokaż/ukryj nakładkę, **Ctrl+Shift+I** — click-through, **Ctrl+Shift+M** — przywróć panel. Po wyborze okna gry: **Ctrl+Shift+T** — cel, **Ctrl+Shift+G** — własna pozycja z kursora nad mapą M. Zamknięcie głównego okna kończy aplikację. Przy zajętym skrócie użyj przycisków panelu; diagnostyka pokazuje stan rejestracji.

## Co nowego w 0.5.1

Poprawki po pierwszym teście w grze:

- Panel broni rozpoznaje profil po amunicji i grupach balistycznie równoważnych, np. `155 mm M185` + `M107 Medium Charge` → M109 (równoważny M109A6). Aliasy znajdują się w `data/weapons/vehicle-aliases.json`.
- Symulacja nie steruje już korektą celowania (**Zakończ symulację**). Kąty mają 2 miejsca po przecinku, a karta korekty jest na górze.
- Mapa nie znika po śmierci ani na ekranie odrodzenia. Inna mapa zastępuje bieżącą dopiero po 2 zgodnych rozpoznaniach. Aplikacja ostrzega o własnej pozycji sprzed odrodzenia, a Automatic zachowuje ostatnie pozycje.
- Automatyczna własna pozycja z białej strzałki na minimapie (beta, domyślnie wyłączona). Kierunek strzałki jest porównywany z kompasem. Fixtures: `tests/fixtures/live-2026-10-16`.

[Raport testów 0.5.1](test-report-v0.5.0.md).

## Co nowego w 0.5

- **Celowanie (gra na żywo):** odczyt panelu stanowiska MTC (broń, amunicja, ELEVATION/AZIMUTH) i korekta `PODNIEŚ/OPUŚĆ`, `W PRAWO/W LEWO` w panelu i na nakładce.
- **Mapa M:** napis „Grid size” identyfikuje mapę z katalogu, siatka 9×9 (A–H, J × 1–9) daje sektory i metry; skróty Ctrl+Shift+T/G ustawiają cel i własną pozycję z kursora.
- **Mapa z katalogu:** auto-skala z wymiaru mapy, teren z heightmap, poprawki kalibracji i terenu.
- **Matcher map:** pełna mapa 0% → 98–100% na syntetycznych klatkach dzięki cechom liczonym raz na referencję ([benchmark](map-matching-benchmark.md)).
- **Broń bezpośrednia / śmigłowiec:** karta nastaw, wyprzedzenie, kreski celownika, strzały kontrolne, tryb „Czołg — szybki” (Numpad +/−/0), rakiety ze śmigłowca, prędkość wylotowa z OCR ekranu Loadout.
- **Balistyka:** salwa MLRS (elipsy 95%), dynamiczne przechwycenie z oporem/napędem (beta), poprawka kroków RK4.
- **Obraz:** poprawka głodzenia klatek WGC, OCR jasnego tekstu kompasu, nagrywanie testowe za zgodą.

Fixtures z prawdziwej gry: `tests/fixtures/live-2026-10-15` (Zone 11, Frontlines). [Raport testów 0.5.0](test-report-v0.5.0.md).

## Gra na żywo (MTC)

Zakładka **Celowanie (gra na żywo)** czyta wyłącznie widoczny obraz okna gry. Nie steruje działem i nie strzela.

Szybki start:

1. **Celowanie (gra na żywo)** → **Odśwież okna** → wybierz okno Roblox → **Czytaj panel broni (2 Hz)**. Widać broń, amunicję oraz ELEVATION/AZIMUTH. Odczyt starszy niż 2 s znika.
2. W grze otwórz mapę klawiszem **M** i kliknij **Rozpoznaj mapę z gry**. Aplikacja wykrywa siatkę i „Grid size”, a następnie wczytuje mapę z katalogu (pierwszy raz zaakceptuj źródło map społeczności). Działo i cel można kliknąć na klatce mapy z gry.
3. Zamiast klikania: najedź kursorem na mapie M na swoją pozycję i naciśnij **Ctrl+Shift+G**, na cel — **Ctrl+Shift+T**. Skrót czyta tylko pozycję kursora i klatkę.
4. Wybierz tor (niski/wysoki). Panel pokazuje korektę, np. `PODNIEŚ +2,3°`, `W LEWO 4,1°` (azymut zawinięty do ±180°); przy |Δ| < 0,1° na obu osiach `✓ NA CELU`. Kliknij **Pokaż nakładkę**, aby widzieć to samo nad grą.

Gdy panel jednoznacznie wskazuje profil z katalogu, pojawia się **Użyj profilu z gry**; nieznana broń daje komunikat o braku profilu. Zgodność konwencji AZIMUTH z azymutem mapy wymaga sprawdzenia w grze. [Czytniki HUD](vision.md#panel-stanowiska-broni-mtc-i-pełna-mapa-fixtures-2026-10-15).

## Automatyka

W zakładce **Automatyka** włącz automatyczne wykrywanie HUD, dobór amunicji i rozpoznawanie mapy. W **Analiza obrazu** wybierz okno i uruchom OCR. Automatyczne pola wymagają dwóch świeżych obserwacji; sam odczyt pojazdu bez amunicji pozostaje niejednoznaczny. Brak jawnych etykiet wymaga ustawienia ROI lub wyboru profilu ręcznie.

Pojedynczą mapę najszybciej wczytuje tryb **Mapa z katalogu**. W **Zasoby map** można wybrać wiele map, opcjonalnie teren, i pobrać pliki online albo wczytać sprawdzony cache offline. Pliki znajdują się w `%LOCALAPPDATA%/MTC-Tactical-Assistant/community-assets-v1`; wydanie ZIP nie zawiera rastrów społeczności o niepotwierdzonej licencji redystrybucji. Odczyt wysokości korzysta ze źródłowego kanału RED8, przelicznika jednostek gry oraz z przygotowanego kwadratowego rastra o boku do 1024 px; niekwadratowe heightmapy są wyśrodkowane w kwadracie mapy, a nie rozciągane. Domyślna siatka 9×9 (kolumny A–H, J, wiersze 1–9 od góry) odpowiada zrzutowi minimapy z 2026-10-15. Dla 4 map źródło nie podaje danych terenu. Można wskazać preferowaną mapę, aby ograniczyć czas wyszukiwania; aplikacja nadal wymaga geometrycznego dopasowania obrazu. [Pochodzenie i cache](community-map-assets.md).

W **Analiza obrazu** podgląd wyboru markerów zachowuje źródłowy czas klatki i wygasa po 2 s. Pierwszy wybór uruchamia odczyt; odśwież podgląd i wskaż drugą rolę. Utrata obserwacji, zmiana okna, mapy lub zatrzymanie wymaga ponownego wskazania ikon. Wybory pozostają wyłącznie w pamięci sesji.

Pełną lokalną mapę dodaj do biblioteki po ustawieniu skali i północy oraz potwierdzeniu źródła. Matcher odnajduje mapę w klatce, obsługuje częściowy kadr i zmianę skali; domyślnie szuka orientacji z północą u góry, inne obroty wymagają konfiguracji. W panelu wyboru markerów pobierz świeży podgląd i wskaż ikonę własnego pojazdu oraz cel. Aplikacja wyznacza kolor i kształt; dwie świeże obserwacje zatwierdzają wybrany komponent także wśród ikon tego samego koloru. Zbyt mała minimapa, podobne referencje lub przekroczenie budżetu czasu wstrzymują pozycje. Pliki map i wysokości są wiązane z kalibracją przez SHA256; zmiana zawartości wymaga ponownego importu.

Mapa wysokości to potwierdzony liniowy obraz szarości z zakresem i jednostkami zgodnymi z konkretną mapą. Import nie dostarcza wysokości celu powietrznego nad terenem. **Profile i balistyka** udostępnia wyprzedzenie 3D z grawitacją, jawną różnicą wysokości i wszystkimi składowymi prędkości. Zwykły wynik bez wysokości zachowuje dystans i azymut, ale nie oblicza elewacji.

W **Automatyka** można dopasować prędkość i grawitację do co najmniej trzech różnych obserwowanych strzałów. Formularz pokazuje reszty, zakres pomiarów i uwarunkowanie. Dopasowanie ma status beta; nie potwierdza naprowadzania ani parametrów aktualnego MTC. [HUD](auto-hud.md), [mapy i teren](auto-maps.md), [amunicja i model 3D](auto-weapons.md).

## Ręczna konfiguracja HUD i pozycji

W **Analiza obrazu** kliknij **Odśwież okna**, wybierz widoczne okno gry i **Pobierz klatkę do kalibracji**. Domyślny backend to Windows Graphics Capture. Błąd WGC jest pokazywany wprost; **GDI — widoczne piksele (fallback ręczny)** wymaga świadomego wyboru. ROI dotyczy obszaru klienta, bez paska tytułu.

Wybierz obszar na liście i przeciągnij jego prostokąt na podglądzie. Włącz potrzebne odczyty: czas, centralna liczba kompasu, prędkość, wysokość oraz jawne napisy pojazdu/broni/mapy. Kilka ROI jest analizowanych w jednej klatce; błąd jednego obszaru nie ukrywa diagnostyki pozostałych. **Rozpocznij OCR** uruchamia lokalną analizę z limitem 0,25–3 FPS. Podgląd i klatki pozostają w pamięci.

Aby uruchomić pozycje z mapy:

1. Otwórz w grze pełny, nieruchomy widok mapy. Zaznacz **Pełna mapa / markery** zgodnie z jego granicami.
2. Rozwiń **Automatyczne pozycje i obserwowany ruch — konfiguracja markerów** i kliknij **Zapamiętaj widok mapy i użyj go jako referencji**. Aplikacja przyjmuje wybrany wycinek jako mapę roboczą i zapisuje mały deskryptor widoku. Ta operacja usuwa poprzednią skalę i punkty.
3. W **Mapa i cel** ustaw skalę, orientację i jednostki tej referencji. Włącz analizę markerów, potwierdź pełny kadr i dobierz rzeczywiste kolory, role oraz progi wielkości ikon. Domyślne kolory są przykładami.
4. W **Assisted** użyj **Przyjmij jednoznaczne punkty w Assisted**, aby zatwierdzić wykryte pozycje. **Automatic** wymaga stabilnych odczytów i jednoznacznie śledzonego markera każdej roli.

Brak referencji, zmiana widoku, obrót/zoom, przycięte ikony, kilka pasujących markerów lub nieaktualne pomiary blokują automatyczne pozycje. Deskryptor sprawdza podobieństwo konkretnego widoku; nie rozpoznaje nazwy mapy ani nie ustala jej skali. Jednolita, pozbawiona tekstury mapa nie zapewnia wiarygodnej referencji. W takim przypadku użyj trybu Manual.

Tracker szacuje ruch własnego markera i wybranego celu z kolejnych świeżych pozycji. Odrzuca skoki i niejednoznaczne obserwacje. W Automatic może dostarczyć prędkość do modelu wyprzedzenia; model własnej prędkości wymaga osobnego włączenia. To przybliżenie ruchu na płaszczyźnie, bez wysokości, przyspieszenia i pełnej mechaniki śmigłowca/przeciwlotniczej. [Konfiguracja i ograniczenia analizy](vision.md), [backend WGC](capture.md).

## Zakres obliczeń i wiarygodność

Działają dwa tory w stałej grawitacji bez oporu, różnica wysokości, matematyczne limity elewacji, czas lotu, prostoliniowe wyprzedzenie oraz ręczna korekta punktu celowania. Opcjonalny rozrzut 95% opisuje pojedynczy pocisk w modelu izotropowego rozkładu normalnego. Prognoza salwy MLRS (elipsy pojedynczego pocisku, obwiedni i środka salwy) wymaga jawnych parametrów rozrzutu. Niepewność pozycji nie obejmuje automatycznie błędu skali, modelu pocisku ani parametrów gry.

Dane społeczności pochodzą z przypiętego commitu publicznego kalkulatora MTC. Ich zgodność z aktualną rozgrywką, fizyką i wersją map **nie została potwierdzona**. Potwierdzenie źródła pozwala wypróbować model, a lokalna kalibracja otrzymuje status beta. Nie zastępujemy brakujących danych parametrami rzeczywistego uzbrojenia. Lokalnie skalibrowane profile beta mogą używać numerycznych modeli oporu liniowego/kwadratowego oraz napędu rakiety w stałym kierunku startowym. Brak współczynników wstrzymuje wynik. Zwykłe obliczenie z tymi modelami obsługuje nieruchomy cel i stanowisko; ruchomy cel liczy osobne iteracyjne przechwycenie (beta). Naprowadzanie pozostaje niedostępne. Wysokość terenu pochodzi wyłącznie z poprawnie importowanej mapy wysokości; brak danych nie jest zastępowany zerem.

Pewność OCR/CV jest heurystyką zgodności i stabilności, nie prawdopodobieństwem zwracanym przez OCR Windows. Automatic stosuje próg 85%. Pomiar starszy niż 2 s znika z prezentacji; WGC wcześniej odrzuca klatki źródłowe starsze niż sekundę. Rozpoznana jawna nazwa nie potwierdza mechaniki broni; opcjonalne dopasowanie profilu wymaga jednoznacznej zgodności napisu z katalogiem.

## Dane lokalne i granice integracji

Aplikacja używa obrazu wybranego okna i danych ręcznych. Nie czyta pamięci Roblox, nie wstrzykuje kodu, nie steruje pojazdem i nie strzela. Nie wysyła klatek. Zapisuje je wyłącznie nagrywanie testowe po zaznaczeniu zgody, do wskazanego katalogu i z limitami ([nagrywanie](capture.md#nagrywanie-testowe-aplikacja)). Zapisuje ustawienia, profile, oznaczenia, deskryptor referencji mapy, historię i log kalibracji. Nakładka jest wykluczana z systemowego przechwytywania tam, gdzie wspiera to Windows.

Ustawienia: `%APPDATA%/pl.mtc.tacticalassistant/settings-v4.json`; wcześniejsze ustawienia v1/v2/v3 są migrowane. Otwarcie wcześniej importowanej mapy może ponownie odczytać wskazany lokalny plik. Mapa pobrana z widocznego kadru pozostaje tylko w pamięci; po ponownym uruchomieniu można ponownie zapamiętać widok i ustawić skalę. Zmiana źródła, ROI lub kalibracji unieważnia wcześniejsze automatyczne obserwacje.

Testy własnego okna i referencji nie potwierdzają działania z każdą konfiguracją Roblox. Czytniki HUD sprawdzono na zrzutach Zone 11 z 2026-10-15; inne rozdzielczości, UI scale, HDR, ekskluzywny fullscreen, fizyczne skróty i przenoszenie między monitorami wymagają dalszej weryfikacji. Aplikacja nie wykrywa przeciwników i nie ma aim-assist. Reguły MTC dotyczące nakładki nie zostały potwierdzone, a zasady Roblox zakazują narzędzi dających nieuczciwą przewagę; projekt nie deklaruje zgodności ani braku ryzyka blokady. [Ograniczenia i źródła](limitations-and-compliance.md).

## Instalacja zależności, budowanie i testy

Sprawdzone środowisko: Windows x64, Rust 1.98.1 MSVC, Visual Studio 18 z C++ Build Tools, Node 22.22.1, npm 11.18.0 i WebView2 155. Zależności są utrwalone w `Cargo.lock` oraz `apps/desktop/package-lock.json`. Polecenia wykonuj z podanych katalogów.

Z katalogu projektu:

~~~powershell
cd apps/desktop
npm.cmd ci
npm.cmd run tauri dev
~~~

Weryfikacja i przygotowanie wydania z katalogu `mtc-tactical-assistant`:

~~~powershell
powershell -ExecutionPolicy Bypass -File scripts/test.ps1
powershell -ExecutionPolicy Bypass -File scripts/build-release.ps1
~~~

`test.ps1` sprawdza formatowanie, Clippy, testy Rust, build okna testowego, build TypeScript/Vite, testy frontendowe i audyt npm. Dla kodu 0.5.0 wykonany przebieg zakończył się powodzeniem: **291 testów Rust, 97 testów TypeScript, Clippy i formatowanie bez błędów, audyt npm bez zgłoszonych podatności**. Status testów konkretnego wydania znajduje się w [raporcie](test-report-v0.5.0.md).

Osobno: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check`; w `apps/desktop`: `npm.cmd run build`, `npm.cmd test`, `npm.cmd audit`.

Okno HUD do testów WGC/OCR:

~~~powershell
cargo build -p mtc-capture --example hud_fixture
target/debug/examples/hud_fixture.exe --multi-hud --seconds 60
~~~

Testy OCR rzeczywiście odczytują czas z obu dostarczonych fotografii. Integracje capture tworzą własne krótkotrwałe okna Win32 i sprawdzają klienta, zmianę rozmiaru, świeżość oraz cykl życia. Testy CV/trackera używają danych syntetycznych.

Test gotowego pliku i stanów okien:

~~~powershell
node apps/desktop/scripts/automation-smoke.mjs release/MTC-Tactical-Assistant-0.5.1.exe
python scripts/generate-auto-fixture.py --output tests/output/v0.5.1/fixtures
node apps/desktop/scripts/map-hud-smoke.mjs --exe release/MTC-Tactical-Assistant-0.5.1.exe
node apps/desktop/scripts/smoke-test.mjs release/MTC-Tactical-Assistant-0.5.1.exe
node apps/desktop/scripts/resources-dynamics-smoke.mjs --exe release/MTC-Tactical-Assistant-0.5.1.exe [--cache-root <community-assets-v1>]
node apps/desktop/scripts/marker-picker-smoke.mjs --exe release/MTC-Tactical-Assistant-0.5.1.exe
node apps/desktop/scripts/live-hud-smoke.mjs --exe release/MTC-Tactical-Assistant-0.5.1.exe
node scripts/test-window-controls.mjs --exe release/MTC-Tactical-Assistant-0.5.1.exe --installer "release/MTC Tactical Assistant_0.5.1_x64-setup.exe" --output-dir tests/windows-output/v0.5.1-api-final --port 9224 --overlay-height 340
~~~

Gotowy `.exe` przechodzi kontrole bazowe, kalibracji/uzbrojenia, map/HUD, zasobów/modeli, wyboru markerów oraz czytników gry na żywo. `live-hud-smoke` sprawdza `read_weapon_panel_file` (także M109), `read_mtc_map_file`, `mtc_map_point`, `read_loadout_specs_file` i `read_own_position_file` na fixtures z 2026-10-15 i 2026-10-16 względem ich `manifest.json`. `resources-dynamics-smoke` wymaga lokalnego cache zasobów (domyślnie `%LOCALAPPDATA%`, inaczej `--cache-root`). Dokładne liczby i SHA256 są w raporcie. Skrypty uruchamiają własny proces z odrębnymi danymi i diagnostycznym WebView2 CDP. Zrzuty własnego interfejsu trafiają do katalogów testowych; zwykła aplikacja nie włącza portu debugowania. `test-window-controls` obsługuje stan nakładki oraz emulację DPI; wariant CLI nie wysyła fizycznych skrótów. Wymaga Python 3.10+, zależności npm i wolnego portu. Audyt NSIS używa 7z/NanaZip i nie wykonuje instalatora. [Procedura Windows i granice testów](windows-validation.md).

Zweryfikowane pakowanie po zakończeniu obu przebiegów:

~~~powershell
python scripts/package-release.py --window-report tests/windows-output/v0.5.1-api-final/window-controls-report.json --allow-unverified-physical-hotkeys
~~~

Opcja zachowuje jawny brak fizycznej weryfikacji skrótów po zatrzymaniu testu klawiszem Esc. Pakowanie nadal wymaga zaliczonego smoke dokładnego EXE, natywnych stanów okien i audytu dokładnego instalatora z licencją. ZIP zachowuje dokumentację, sumy i raporty, a wcześniejsze wydania pozostają w katalogu.

## Struktura projektu

`apps/desktop` — panel, nakładka i komendy Tauri; `crates/{maps,profiles,ballistics,capture,vision,assets}` — biblioteki Rust; `data` — lokalny katalog z provenance i licencją źródła; `tests/fixtures` — referencje; `scripts` — testy, import i wydanie; `docs` — architektura, kalibracja, ograniczenia oraz wyniki.

Kolejny etap wymaga sprawdzenia trafień dla wybranych pocisków w aktualnym MTC. Pozostały też pomiary długotrwałej wydajności, rzeczywista macierz DPI/monitorów, fizyczne testy skrótów, test instalacji i podpis cyfrowy. Parametry oporu i napędu wymagają pomiarów w grze, a model naprowadzania pozostaje do wykonania. Korekta celowania i czytniki wymagają porównania z trafieniami w aktualnym MTC.

