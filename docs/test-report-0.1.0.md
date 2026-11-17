# Raport testów wydania 0.1.0

**Data:** 13 października 2026. **Platforma:** Windows x64, WebView2 155, DPI 125%. Przetestowano rzeczywiście uruchomioną aplikację z `release/MTC-Tactical-Assistant-0.1.0.exe` — 13 273 088 bajtów; SHA-256 `953A8EBE7C8AA923F70526311A603155852AA31307F6B116D0A3484134E68307`.

## Wyniki

| Weryfikacja | Wynik |
|---|---|
| Rust: ballistics | 6/6 |
| Rust: maps | 6/6 |
| Rust: profiles | 3/3 |
| Rust: vision — walidacja, stabilność, świeżość | 5/5 |
| Windows: capture własnego widocznego okna i zniszczony uchwyt | 1/1 |
| Windows OCR: obie dostarczone fotografie | 2/2 |
| Łącznie testy Rust | **23/23** |
| TypeScript: świeżość, import i uszkodzona konfiguracja | **7/7** |
| Smoke test uruchomionego EXE | **12 kontroli zakończonych sukcesem** |
| `cargo fmt --all --check` | sukces |
| `cargo clippy --workspace --all-targets -- -D warnings` | sukces |
| `npm.cmd run build` — TypeScript i Vite | sukces |
| `npm.cmd audit` — wszystkie zależności npm | 0 zgłoszonych podatności |
| Tauri release x64 | zbudowany i uruchomiony |
| Instalator NSIS x64 | zbudowany; bez testu instalacji w profilu użytkownika |

Pełna weryfikacja jest powtarzalna przez `scripts/test.ps1`. Natywny smoke test: `node apps/desktop/scripts/smoke-test.mjs release/MTC-Tactical-Assistant-0.1.0.exe`. Przed nim potrzebny jest helper `cargo build -p mtc-capture --example hud_fixture`; pełny skrypt testowy buduje helper automatycznie.

## Co sprawdził uruchomiony EXE

1. Wczytanie siedmiu profili przez natywne IPC.
2. Wynik Rust dla przykładu: dystans **502,5 jednostki**, azymut **58,8°**, niski tor około **15,08°**, czas lotu **5,20 s**. Parametry syntetyczne v=100, g=10; nie są danymi MTC.
3. Widoczność, always-on-top i brak ramki nakładki oraz rejestracja wszystkich trzech skrótów globalnych.
4. Przełączenie rzeczywistej flagi Win32 `WS_EX_TRANSPARENT` w obie strony. Weryfikacja flagi przez osobny proces testowy.
5. Przekazanie wyniku do drugiego WebView, prezentacja nakładki i przezroczyste tło HTML.
6. OCR fotografii BM-21: **27:42**, 11 ms w ostatnim smoke teście.
7. OCR fotografii śmigłowca: **29:33**, 5 ms.
8. Odczyt referencji i podgląd ROI przez interfejs aplikacji.
9. Wygaszenie starego odczytu HUD po 2 s.
10. Pełny tor **widoczne okno Windows → GDI capture → Windows OCR → IPC**: **27:42**, 49 ms etapu OCR. Okno zostało wygenerowane przez helper testowy; nie jest to dowód poprawności w każdej konfiguracji Roblox.
11. BM-21 bez stałych blokuje elewację, zachowując dystans i azymut na skalibrowanej mapie.
12. Wersjonowany zapis ustawień w wydzielonym katalogu testowym.

Czasy są pojedynczymi odczytami po rozgrzaniu silnika i obejmują analizę ROI, nie cały koszt przechwytywania. Nie stanowią długotrwałego benchmarku CPU/GPU. Surowy wynik: `tests/output/smoke-test.json`. Zrzuty własnego interfejsu: `desktop-map.png`, `desktop-ocr.png`, `overlay.png` w tym samym katalogu. Nie nagrywano rozgrywki ani nie zapisywano ekranu użytkownika poza interfejsem testowanego programu.

## Kryteria i granice akceptacji

**Etap 1:** dostępny uruchamialny EXE, nakładka, wybór profilu, konfiguracja mapy, pozycja/cel, poprawny dystans i azymut, elewacja blokowana bez stałych, symulacja i testy. Spełnione dla prototypu.

**Etap 2:** rzeczywiste przechwytywanie Windows i co najmniej jeden wiarygodny odczyt HUD z obrazu. Tor capture/OCR sprawdzony na własnym oknie, OCR fotografii sprawdzony na obu referencjach. Próba w rzeczywistym oknie MTC, różne UI scale i zasłonięcia pozostają do walidacji.

**Etapy 3–4:** import/kalibracja map, oznaczenia, korekta i historia, profile wszystkich kategorii są dostępne. Automatyczne rozpoznawanie map/ikon/pozycji, śledzenie celu i stałe konkretnych broni nie są ukończone. Rakiety kierowane mają uczciwy stan braku modelu.

**Etap 5:** wydanie lokalne i instrukcja dostępne. Nie wykonano podpisania, fizycznego testu każdego skrótu klawiaturowego, testów 100/150/200% DPI, przenoszenia między monitorami, długotrwałego benchmarku ani sprawdzenia instalatora przez instalację. Zwykłe uruchomienie nie włącza portu debugowania używanego w testach.

## Artefakty

- `release/MTC-Tactical-Assistant-0.1.0.exe` — 13 273 088 bajtów; SHA-256 `953A8EBE7C8AA923F70526311A603155852AA31307F6B116D0A3484134E68307`.
- `release/MTC Tactical Assistant_0.1.0_x64-setup.exe` — 3 556 816 bajtów; SHA-256 `D455FE5A6AE3D06F9D413869ED4AAFFE688C3622BCB0A39EF04B1F930034BA7E`.
- `release/SHA256SUMS.txt` — sumy kontrolne wykonywalnych plików wydania.

Oba pliki są niepodpisane. Parametry balistyczne MTC i reguły nakładek pozostają niepotwierdzone; wniosek o sprawności prototypu nie jest zapewnieniem skuteczności wskazań w aktualnej grze.
