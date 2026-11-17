# Raport testów wydania 0.5.0

Data: 16 października 2026. Windows 11 x64, rzeczywiste DPI 125%, WebView2. Testy używają własnych procesów z odrębnymi danymi, syntetycznych okien HUD, prywatnej kopii cache publicznych map i wycinków zrzutów gry z 2026-10-15. Wszystkie raporty końcowe wskazują identyczny SHA256 portable EXE.

| Artefakt | SHA256 |
|---|---|
| `MTC-Tactical-Assistant-0.5.0.exe` | `72fc2b326d80ce4e6e736573214f95ecd1154ed496e1b61e5a938a25c8563863` |
| `MTC Tactical Assistant_0.5.0_x64-setup.exe` | `215b594add79a249c6762011d8cfb0125aa7f7933cd6bd22834775f4e139627b` |

Sumę ZIP zawiera `release/release-0.5.0.json`, a sumy obu plików `.exe` — `SHA256SUMS-0.5.0.txt`.

## Kod

`scripts/test.ps1` zakończył się kodem 0: **291 testów Rust i 97 testów TypeScript**, formatowanie, Clippy z `-D warnings`, build interfejsu i okna testowego oraz audyt npm bez podatności (`tests/output/v0.5.0/code-checks.log`, `unit-test-summary.json`).

Testy Rust obejmują fixtures z gry: 8 paneli broni (nazwy, ELEVATION/AZIMUTH z dokładnością 0,01°, RANGE „0000” jako brak pomiaru), pełną mapę Zone 11 (siatka ±3 px, „Grid size: 324 meters”, sektory A5/G5), timer 53:36, kompas 269 i dwa ekrany Loadout.

## Weryfikacja gotowego EXE

Raporty: `tests/output/v0.5.0`. Wszystkie kontrole przeszły na EXE `72fc2b32…`.

| Test | Kontrole | Zakres |
|---|---:|---|
| `smoke-test` | 19 | IPC, symulacja, nakładka i click-through Win32, OCR fotografii, WGC, markery, Automatic, unieważnienie po zamknięciu okna |
| `automation-smoke` | 24 | Katalog 153 profili, resolver nazw, dopasowanie v/g z pomiarów, przechwycenie 3D, formularze CSV i 3D |
| `map-hud-smoke` | 13 | Mapa i markery w pełnej klatce, teren, bramki SHA/mapy/kalibracji, Automatic z UI |
| `resources-dynamics-smoke` | 27 | Cache 2 map i terenów, RED8, modele vacuum/drag/rocket, dynamiczne przechwycenie beta, formularze |
| `marker-picker-smoke` | 7 | Wybór ikon kliknięciem, ten sam kolor, odrzucenie zmanipulowanych i nieświeżych wyborów |
| `live-hud-smoke` (nowy) | 14 | `read_weapon_panel_file` ×8, `read_mtc_map_file`, `mtc_map_point`, kontrole negatywne, `read_loadout_specs_file` ×2, zakładka **Celowanie (gra na żywo)** |

Razem 104 kontrole. `live-hud-smoke` porównuje wyniki z `tests/fixtures/live-2026-10-15/manifest.json`. Panel jest wklejany w klatkę 1920×1080 na pozycji z manifestu. UB-32 / S-5K1 rozpoznaje profil `community-ub32-1`, a Rocketetet i 73 mm 2A28 nie dostają zgadywanego profilu. Klatki pośrednie zostają w katalogu testu i nie trafiają do ZIP.

Zmiany skryptów względem 0.4: `resources-dynamics-smoke` oczekiwał odrzucenia `calculate` z ruchem 3D i aktywnym oporem. Od 0.5 obliczenie zachowuje azymut, nie zwraca wyniku vacuum i odsyła do `dynamic_intercept`. Test sprawdza teraz to zachowanie oraz trafienie ruchomego celu przez `dynamic_intercept`, odtworzone natywną symulacją. Wymaga też jawnego wyboru dziedziczenia prędkości stanowiska. `marker-picker-smoke` czeka na zwolnienie natywnego slotu klatki, zamiast ścigać się z pętlą UI.

## Poprawka aplikacji

`marker-picker-smoke` nie przechodził na pierwszym buildzie 0.5. **Odśwież podgląd markerów** podczas działającego OCR kończyło się błędem „Trwa analiza klatki…”. Od poprawki głodzenia WGC `analyse_batch` czeka na nową klatkę i trzyma natywny slot prawie przez cały interwał. Pętla UI zgłaszała kolejne żądanie natychmiast, więc podgląd (czeka ≤ 800 ms) nie dostawał slotu.

Poprawka w `App.tsx` i `vision-utils.ts`: pętla zostawia co najmniej 30 ms przerwy między skanami. Odrzucenie „Poprzednia klatka jest przetwarzana” nie kasuje już ostatniej obserwacji, a bramka świeżości 2 s nadal obowiązuje. Testy: `vision-utils.test.ts`.

Po poprawce `marker-picker-smoke` przeszedł 4 z 5 przebiegów. W jednym oba wybory przeszły w stan „stale” po drugim kliknięciu. Przyczyny nie ustalono. Zapisany raport pochodzi z przebiegu zaliczonego na tym samym EXE.

## Windows i instalator

`tests/windows-output/v0.5.0-api-final/window-controls-report.json`: 5 natywnych stanów nakładki i 16 przypadków emulacji skalowania 100–200%, bez przepełnienia poziomego i przyciętych kontrolek. Trzy skróty nakładki są zarejestrowane. Audyt rozpakowuje instalator NSIS i porównuje payload z portable EXE oraz licencją. Instalatora nie uruchomiono. Pakowanie użyło `--allow-unverified-physical-hotkeys`: nie testowano fizycznych klawiszy, w tym Ctrl+Shift+T/G i Numpad +/−/0.

## Granice wyniku

- Testy nie potwierdzają trafień ani aktualnej mechaniki MTC.
- Czytniki sprawdzono na jednej sesji: 1920×1080, DPI 125%, Zone 11.
- Nie sprawdzono konwencji AZIMUTH względem mapy ani działania korekty celowania w rozgrywce.
- Brak testu live: odczytu panelu z okna Roblox, skrótów mapy na otwartej mapie M i nagrywania w grze.
- Profile społeczności pozostają eksperymentalne, a opór/napęd — lokalną betą.
- Do wykonania pozostają: podpis cyfrowy, instalacja oraz fizyczne monitory i klawisze.

Raport wydania 0.4: [archiwum](test-report-v0.4.0.md). Raport bieżącego wydania: [test-report.md](test-report-v0.5.0.md).
