# Raport testów wydania 0.4.0

Data: 14 października 2026. Windows x64, rzeczywiste DPI 125%, WebView2. Testy używają własnych procesów, syntetycznych okien HUD oraz prywatnego cache publicznych map. Wszystkie raporty końcowe muszą wskazywać identyczny SHA256 portable EXE.

SHA256 portable EXE: `d016778016b6d21a3fdc2862ddbb55f7d7e03886c69a007d992c9dbe063f848c`.

## Kod

`scripts/test.ps1` zakończył się kodem 0: **180 testów Rust i 32 testy TypeScript**, formatowanie, Clippy z `-D warnings`, build interfejsu, build okna testowego i audyt npm bez zgłoszonych podatności. Nowy test dziesięciokrotnego zapisu/odczytu skal rzeczywistych map zachowuje dokładne bity f64 dzięki `serde_json/float_roundtrip`. Zapobiega to odrzuceniu poprawnej kalibracji po przejściu przez JSON.

Nowe moduły obejmują 25 publicznych tekstur i 21 heightmap, bezpieczne pobieranie/cache, przygotowanie RED8, bibliotekę wielu terenów, wskazywanie ikon i niezależne śledzenie wybranych komponentów oraz numeryczne modele oporu i napędu. Dwa wybrane ślady nie mogą używać tego samego widocznego komponentu.

## Weryfikacja gotowego EXE

Końcowe wyniki i sumy znajdują się w `tests/output/v0.4.0`: `smoke-test.json`, `automation-smoke.json`, `map-hud-smoke.json`, `resources-dynamics-smoke.json` i `marker-picker-smoke.json`. Pierwsze trzy obejmują 19 +24 +13 kontroli. Test zasobów/modeli obejmuje 25 kontroli: rzeczywisty cache, niezależny odczyt RED8, wiele terenów, pochodzenie/SHA, model vacuum/drag/rocket, budżety, brak danych oraz prawdziwe formularze i podgląd trajektorii. Parametry balistyki testowej są syntetyczne i nie stanowią kalibracji MTC.

`normal-resources-setup.json` potwierdza preload i zapis map w zwykłych ustawieniach. Instalacja tych zasobów wynika z zaakceptowania przez użytkownika wcześniejszego źródła społeczności. Istniejące wpisy i kalibracje są zachowywane. Rastry nie trafiają do ZIP, ponieważ licencja ich redystrybucji nie została potwierdzona.

## Windows i instalator

`tests/windows-output/v0.4.0-api-final/window-controls-report.json` obejmuje 5 natywnych stanów nakładki i 16 przypadków emulacji skalowania. Audyt rozpakowuje instalator NSIS i porównuje jego payload oraz licencję z dokładnym portable; instalatora nie wykonano. Wszystkie trzy skróty są rejestrowane. Fizycznych skrótów ani mieszanej macierzy monitorów nie testowano.

## Granice wyniku

Testy nie potwierdzają bieżącej mechaniki/HUD Roblox. Publiczne mapy i 153 profile pozostają eksperymentalne. Współczynniki oporu/napędu wymagają lokalnej kalibracji beta; solver obsługuje w tym modelu nieruchomy cel i stanowisko. Naprowadzanie, dynamiczne przechwycenie z oporem/napędem, podpis cyfrowy i fizyczne testy instalacji/monitorów pozostają do wykonania. Brak lub utrata obserwacji wstrzymuje wynik. Aplikacja nie steruje pojazdem i nie automatyzuje strzału.

Raport wydania 0.3: [archiwum](test-report-v0.3.0.md). Raport bieżącego wydania: [test-report.md](test-report-v0.5.0.md).
