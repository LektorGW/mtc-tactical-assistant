# Realizacja wymagań — wersja 0.5.1

Macierz odnosi się do [oryginalnej specyfikacji](original-specification.md). „Zaimplementowane” oznacza istniejący kod i podany zakres walidacji; nie potwierdza aktualnych parametrów MTC. Liczby testów i wyniki smoke testów gotowego pakietu opisuje [raport testów](test-report-v0.5.0.md).

| Wymóg specyfikacji | Stan 0.5.1 | Dowód i granice |
|---|---|---|
| Aplikacja Windows, polski panel, osobna nakładka | Zaimplementowane | Tauri/React/Rust, portable/NSIS i skrypty wydania; raport dokładnego EXE |
| Przezroczystość, always-on-top, click-through, przesuwanie i skróty | Zaimplementowane; część walidacji warunkowa | Natywny stan i emulacja DPI sprawdzone; fizyczne skróty i monitory niezweryfikowane |
| Publiczne przechwytywanie bez ingerencji w grę | Zaimplementowane | WGC (z poprawką głodzenia klatek przy niskim FPS), jawny GDI, crop klienta i cykl życia na własnym oknie |
| OCR wielu obszarów i podgląd diagnostyczny | Zaimplementowane | Timer, kompas (także jasny tekst), liczby i jawne napisy; OCR fotografii i zrzutów gry 2026-10-15 |
| Panel stanowiska broni MTC (nazwa, amunicja, ELEVATION/AZIMUTH) | Zaimplementowane / eksperymentalne | Wzorce LED, stabilizacja dwóch klatek, RANGE „0000” = brak pomiaru; 8 paneli z gry w testach Rust i smoke EXE; inne rozdzielczości i bronie niesprawdzone |
| Korekta celowania na żywo („PODNIEŚ/W PRAWO”) | Zaimplementowane / eksperymentalne | Zakładka **Celowanie (gra na żywo)** i nakładka. Wymagane − bieżące, kąty z 2 miejscami, wiek 2 s. Symulacja nie steruje korektą. Tylko tekst, bez sterowania; konwencja AZIMUTH i trafienia niezweryfikowane |
| Pełna mapa M i siatka sektorów | Zaimplementowane | „Grid size” → identyfikacja mapy z katalogu, siatka 9×9 A–H, J × 1–9 potwierdzona zrzutem Zone 11; niejednoznaczny rozmiar pola wymaga wyboru |
| Trwałość mapy po śmierci i odrodzeniu | Zaimplementowane | Utrata obserwacji nie czyści mapy. Inna mapa zastępuje bieżącą dopiero po 2 zgodnych rozpoznaniach (`auto-map.ts`, testy). Ostrzeżenie o własnej pozycji sprzed odrodzenia; Automatic zachowuje ostatnie pozycje |
| Automatyczna własna pozycja z minimapy | Beta, domyślnie wyłączona | Biała strzałka gracza w siatce minimapy, kierunek porównany z kompasem ±20° (`ready`/`unconfirmed`). Na 28 oznaczonych klatkach Rohkstov: 26 trafień, 0 fałszywych pozycji. Pojazdy, mapa M z piechotą i inne mapy niezweryfikowane |
| Skróty punktów mapy Ctrl+Shift+T/G | Zaimplementowane; fizycznie niezweryfikowane | Odczyt pozycji kursora nad otwartą mapą M → cel / własna pozycja; bez klikania i wysyłania wejścia |
| Rozpoznanie pojazdu/uzbrojenia z jawnej nazwy | Warunkowe / eksperymentalne | Etykiety HUD lub panelu broni. Dokładna amunicja i grupy balistycznie równoważne (np. 155 mm M185 + M107 Medium Charge → M109, równoważny M109A6; aliasy w `vehicle-aliases.json`). Różne parametry dają stan niejednoznaczny, a nieznana broń — brak profilu. Brak identyfikacji po sylwetce |
| Manual / Assisted / Automatic | Zaimplementowane | Ręczne punkty, akceptacja markerów, stabilizacja i freshness; Automatic blokuje niekompletne dane |
| Mapy, współrzędne, skala, obrót, cel i korekta | Zaimplementowane | **Mapa z katalogu** (auto-skala z wymiaru, teren z heightmap) i własny obraz z kalibracją A–B |
| Rozpoznanie widoku mapy oraz własnej pozycji i celu | Warunkowe / eksperymentalne | Matcher 98–100% pełnej mapy na syntetycznych klatkach, prawdziwy zrzut Zone 11 rozpoznany; minimapa 20–40% ([benchmark](map-matching-benchmark.md)) |
| Widoczne markery i ruch celu/stanowiska | Warunkowe / eksperymentalne | Ikony wskazane przez użytkownika, tracker świeżych pozycji; brak wykrywania przeciwników |
| Dystans, azymut, elewacja, czas lotu | Zaimplementowane z bramką danych | Solwer stałej grawitacji i RK4 (poprawione kroki brzegowe); parametry lokalne lub potwierdzone eksperymentalne |
| Rozrzut, salwa i niepewność | Zaimplementowane / beta | Elipsy 95% pojedynczego pocisku, obwiednia i środek salwy MLRS z jawnych parametrów rozrzutu; brak danych blokuje wynik |
| Wiele rodzajów uzbrojenia i profile JSON | Zaimplementowane; mechaniki warunkowe | Osiem kategorii z `direct_fire`, 153 profile społeczności / 51 nazw, status i provenance |
| Czołgi i ogień bezpośredni | Zaimplementowane / eksperymentalne | Karta nastaw, wyprzedzenie iteracyjne, kreski celownika, strzały kontrolne, tryb „Czołg — szybki” (Numpad +/−/0); prędkość z OCR Loadout; jednostki i grawitacja to założenia |
| Śmigłowiec: działka i rakiety | Częściowo / eksperymentalne | Trafienie rakiety z nurkowania i kąt dla dystansu; start wzdłuż wektora lotu i dziedziczenie prędkości do weryfikacji; brak pełnej dynamiki lotu |
| Przeciwlotnicze i przechwycenie | Warunkowe / beta | Przechwycenie 3D vacuum oraz iteracyjne z oporem/napędem; stała prędkość celu, bez przyspieszeń |
| Katalog map i wysokość terenu | Zaimplementowane z bramką danych | 25 map, 21 heightmap, cache z SHA256; poprawki RED8 i wyśrodkowania niekwadratowych terenów |
| Opór i napęd rakiet | Zaimplementowane / lokalna beta | Jawne współczynniki, RK4, statyczne i dynamiczne celowanie; brak automatycznego dopasowania współczynników |
| Naprowadzanie | Odłożone | Brak obserwowanego modelu; guided zablokowany |
| Dopasowanie modelu z obserwowanych strzałów | Zaimplementowane / beta | ≥3 pomiary (artyleria) lub ≥2 strzały kontrolne (vacuum, ogień bezpośredni); nie zastępuje walidacji gry |
| Lokalne ustawienia, log, historia i eksport/import oznaczeń | Zaimplementowane | Wersjonowane ustawienia v4 i lokalne JSON; współpraca LAN odłożona |
| Testowe nagrania za zgodą | Zaimplementowane | Zgoda przed każdym nagraniem, katalog wybrany przez użytkownika, limity i wskaźnik; analiza offline nagrań |
| Kontrola CPU/GPU, FPS, stale i utrata obrazu | Zaimplementowane; benchmark do dalszej walidacji | Throttle readbacku, jedna najnowsza klatka, czas źródłowy; brak długotrwałego pomiaru w grze |
| Lokalizacja PL, dokumentacja, testy i regresje | Zaimplementowane | Dokumentacja modułów, testy deterministyczne, fixtures z gry |
| Pełna macierz monitorów/DPI, instalacja i podpis | Częściowo / odłożone | Emulacja i audyt instalatora; fizyczne scenariusze, instalacja i podpis niepotwierdzone |
| Aktualna gra, HUD i regulamin MTC | Częściowo / niepotwierdzone | Czytniki sprawdzone na zrzutach Zone 11; brak zgody twórców na nakładkę ([zasady](limitations-and-compliance.md)) |
| Brak pamięci gry, injection, firing, sterowania i aim-assist | Zachowana granica | Wyłącznie obraz okna i dane ręczne; brak wykrywania przeciwników i wysyłania wejścia |

Etapy 1–4 mają działający core, przechwytywanie, OCR, kalibrację, markery i wszystkie kategorie z jawnym stanem wsparcia. Etap 5 ma procedury wydania. Do wykonania pozostają: walidacja trafień w aktualnym MTC, fizyczne monitory i skróty, instalacja oraz podpisane wydanie.

[Schemat profili](weapon-profiles.md), [kalibracja](calibration.md), [analiza obrazu](vision.md), [WGC](capture.md), [ograniczenia i zasady](limitations-and-compliance.md).
