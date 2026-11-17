# Raport testów wydania 0.2.0

Data: 13 października 2026. Windows x64, WebView2 155, rzeczywiste DPI 125%. Dane testowe są oddzielone od zwykłych ustawień aplikacji. Raport 0.1 zachowano w [archiwum](test-report-0.1.0.md).

## Kod i biblioteki

Końcowy przebieg `powershell -ExecutionPolicy Bypass -File scripts/test.ps1` zakończył się kodem 0 po poprawkach WGC i widoczności nakładki.

| Zakres | Wynik |
|---|---|
| Rust: ballistics | 8/8 |
| Rust: maps | 6/6 |
| Rust: profiles | 7/7 |
| Rust: vision — parser, multi ROI, CV i tracker | 29/29 |
| Windows OCR dostarczonych fotografii | 2/2 |
| Rust: capture — jednostkowe | 11/11 |
| Windows capture GDI i WGC — integracje | 2/2 |
| Łącznie Rust | **65/65** |
| TypeScript — konfiguracja, migracja i świeżość | **10/10** |
| Formatowanie i Clippy z `-D warnings` | sukces |
| TypeScript / Vite build | sukces |
| Audyt npm | 0 zgłoszonych podatności |

Profil społecznościowy BM-21 jest sprawdzany względem matematycznego modelu źródła. Test nie potwierdza skuteczności w aktualnej grze. Testy blokują dane bez źródła/parametrów, niepotwierdzone profile, naprowadzanie, złą geometrię, brak wzorca mapy, niejednoznaczne markery, skoki, przyszłe i stare pomiary. Ustawienia v1 migrują do v2.

## Odporność WGC

Probe na własnym pasywnym oknie przeszedł **40 cykli start–stop / 80 klatek / 0 błędów**, exit 0. Odtworzył rzeczywiste źródła o około 11–13 ms wyprzedzające QPC. Klatka jest wstrzymywana najwyżej 200 ms albo odrzucana; nie zmieniamy jej źródłowego czasu. Przyszły pomiar nie kończy całej sesji.

Test restartów wykrył AV przy ponownej aktywacji fabryki WinRT po zamknięciu apartamentu. Fabryki są teraz lokalne dla sesji; ochronę D3D i czas życia odebranych klatek również uporządkowano. Dowody i granice: [capture](capture.md), `tests/output/wgc-clock-probe-summary.json` i log churn. To ograniczony przebieg na tym komputerze, bez gwarancji dla każdego sterownika.

## Gotowy EXE i nakładka

Końcowy build Tauri x64 i smoke test gotowego pliku zakończyły się kodem 0. **19/19 kontroli przeszło** na finalnym `release/MTC-Tactical-Assistant-0.2.0.exe`. Wynik: `tests/output/v0.2.0/smoke-test.json`, SHA256 `0536AEB1CCF45513B9A8B27A47DC7D13124B3B138118F63B43CBC4FBA22384F8`. Raport zapisuje także PID i zdarzenia zamknięcia/crash WebView; w zaliczonym przebiegu zdarzeń tych nie było.

Rozwinięty smoke obejmuje katalog 160 profili i 25 map, syntetyczne obliczenia Rust, rzeczywiste flagi Windows nakładki, click-through zachowujący widoczność, eventy między WebView, OCR obu zdjęć, wygaszenie pomiarów, GDI oraz WGC na własnym oknie. Multi ROI odczytuje timer `27:42`, kompas `138` i napis `BM-21 Grad`; markery w referencji prowadzą do dystansu około `214,7` i azymutu `117,8°`. UI Automatic musi wyliczyć rozwiązanie ze stabilnych markerów i usunąć je po zamknięciu źródła. Źródło jest oznaczone jako okno testowe.

W ostatnim przebiegu OCR zdjęć trwał około 52,5 ms i 41,4 ms, a analiza batch własnego okna 31,1 ms. Są to pojedyncze pomiary, bez pełnego kosztu przechwytywania i bez długotrwałego benchmarku CPU/GPU. Zrzuty są w `tests/output/v0.2.0`; w paczce ZIP odpowiada im `verification`. Aplikacja poza testem nie włącza CDP.

Fizyczne testy wcześniejszej wersji wykryły przejmowanie fokusu i regresję ukrywania przy click-through. Poprawka używa cache Tauri oraz pasywnej nakładki `focusable=false`. Ponowny Computer Use użytkownik zatrzymał klawiszem Esc przed wysłaniem klawiszy. **Fizyczne skróty końcowego binarium oraz M po minimalizacji pozostają niepotwierdzone.** Końcowy audyt przez API przeszedł **5/5 stanów Windows oraz 16 przypadków emulacji DPI**; rejestracja skrótów 3/3, rzeczywiste DPI 125%. Pusty stan nakładki 390×340 logicznych px mieści się przy 100/125/150/200%. Przy stałym rozmiarze fizycznym pozostaje pionowy overflow, jawnie oznaczony jako stress test. [Szczegóły Windows](windows-validation.md).

## Dane, artefakty i pozostałe granice

153 profile pocisków / 51 nazw uzbrojenia, 7 szablonów i wymiary 25 map pochodzą z lokalnego snapshotu oraz [przypiętego źródła](community-sources.md). Import `--offline` odtworzono; brak automatycznej zgody na parametry. Licencja MIT i manifest są zasobami instalatora i pakietu.

Zbudowane artefakty: `release/MTC-Tactical-Assistant-0.2.0.exe` i `release/MTC Tactical Assistant_0.2.0_x64-setup.exe`. Audyt instalatora potwierdził PE x64 payloadu, zgodność z portable po uwzględnieniu trzybajtowego markera Tauri i pełną licencję MIT. SHA256 instalatora: `AEDED97ABC14EEACC0DBA452121F04BA0F7E5CAAB353D7223DE79DA48E78F8B1`.

Pakiet `release/MTC-Tactical-Assistant-0.2.0-Windows-x64.zip` zawiera oba EXE, dokumentację, licencję, manifest, sumy i dowody. W ZIP pliki wykonywalne są obok README; `verification` zawiera raporty i obrazy własnego interfejsu. Sumy: `release/SHA256SUMS-0.2.0.txt`; metadata pakietu: `release/release-0.2.0.json`. Skrypt pakowania sprawdza zgodność hashy z finalnym smoke i audytem oraz CRC całego ZIP; `physicalHotkeysVerified` pozostaje false.

Nie wykonywano instalatora w profilu użytkownika ani podpisania cyfrowego. Emulacja 100/125/150/200% nie zastępuje natywnego przejścia między monitorami; stan przy ograniczonym rozmiarze fizycznym jest testem przeciążeniowym. Aktualny HUD MTC, trafienia, długotrwały CPU/GPU, HDR i fullscreen nadal wymagają walidacji. Brak heightmap, drag, napędu rakiet, naprowadzania i nagrywania dyskowego. Nie przechwytywano ani nie zapisywano rozgrywki w tych testach.
