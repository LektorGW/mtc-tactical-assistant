# Raport testów wydania 0.3.0

Data: 14 października 2026. Windows x64, rzeczywiste DPI 125%, WebView2. Testy korzystają z odrębnych ustawień i własnych okien. Wszystkie trzy testy aplikacji oraz audyt instalatora dotyczą dokładnie tego samego pliku portable.

SHA256 EXE: `acdf3fc935f74b151650e4020fd9f423ad52d55ea5bd2e6e6b6c9e98f107d46c`.

## Kod i biblioteki

Końcowy `powershell -ExecutionPolicy Bypass -File scripts/test.ps1` zakończył się kodem 0. Wyniki: **130 testów Rust, 15 testów TypeScript**, formatowanie i Clippy z `-D warnings`, build interfejsu oraz audyt npm z zerową liczbą zgłoszonych podatności. Po poprawce zegara powtórzono build i 15 testów frontendu.

Rust: ballistics 19, profiles 14, maps 25, capture 13, vision 53, integracja natywnej aplikacji 6. OCR rzeczywiście odczytuje czas z obu dostarczonych fotografii, także z automatycznie znalezionych obszarów. Fotografie nie dają dowodów jawnej nazwy pojazdu lub amunicji.

Sprawdzono brak skali i wysokości, identyfikację amunicji, jednostki, reszty kalibracji, rekonstrukcję torów 3D, nieznane naprowadzanie, geometrię map, niejednoznaczność, brak terenu, zmiany źródła, przyszłe i stare pomiary. Brak wysokości pozostawia dystans i azymut, bez wymyślonej elewacji.

## Gotowy plik Windows

- Test bazowy: **19/19** — IPC, dane katalogowe, obliczenia, WGC/OCR, markery, tryb Automatic, nakładka i utrata źródła.
- Kalibracja i uzbrojenie: **24/24** — wszystkie 153 pełne nazwy, niejednoznaczny sam pojazd, dopasowanie prędkości/grawitacji, blokady danych, rekonstrukcja 3D i rzeczywiste formularze CSV → wynik → zapis beta.
- Automatyczne mapy i HUD: **13/13** — wykrycie siedmiu pól, konkretny pocisk, referencja w całej klatce, marker → mapa, ruch, terrain ID/jednostki/kalibracja, zmieniony SHA256, reset historii po stop i rzeczywisty przepływ UI.

Łącznie **56 zaliczonych kontroli działania EXE**, bez oczekujących przypadków. Matcher rozpoznał testową mapę w obecności HUD i markerów; zamknięcie źródła unieważniło wynik. Te próby wykorzystują syntetyczne okno i potwierdzają działanie integracji. Nie stanowią testu celności w aktualnym Roblox.

Raporty JSON oraz obrazy są w `tests/output/v0.3.0`, a w ZIP w katalogu `verification`. Raporty zawierają SHA256, payloady i informacje o zakończeniu procesów. Skrypty testowe oczekują na rzeczywiste wyniki IPC przez polling z `await`; nie traktują samego Promise jako spełnionego warunku.

## Nakładka i instalator

Kontrola Windows: **5/5 natywnych stanów okien**, w tym rzeczywiste `WS_EX_TRANSPARENT`, widoczność, always-on-top i brak dekoracji. Zarejestrowano trzy skróty globalne. Wykonano 16 przypadków emulacji rozmiarów/DPI dla pustej i wypełnionej nakładki; stres stałego fizycznego okna przy zwiększonym zoomie jest opisany osobno w raporcie.

Nie wysyłano fizycznych klawiszy. Ich działanie pozostaje niezweryfikowane po wcześniejszym zatrzymaniu Computer Use klawiszem Esc. Emulacja nie zastępuje testu fizycznych monitorów. Instalatora nie uruchamiano: rozpakowano i sprawdzono x64 PE, zgodność payloadu z portable z pominięciem znacznika bundle Tauri oraz pełną licencję źródła MIT. Aplikacja pozostaje niepodpisana cyfrowo.

## Pozostałe granice

Automatyka wymaga widocznych danych, skonfigurowanych ról/kolorów markerów oraz lokalnych skalibrowanych referencji map. Publiczny katalog dostarcza wymiary 25 map, bez lokalnych tekstur i heightmap. Mała minimapa w bardzo teksturowanej scenie może pozostać nierozpoznana. Teren nie określa wysokości celu powietrznego. Opór, napęd rakiet, naprowadzanie, pełna korelacja salwy i kompletna propagacja niepewności pozostają poza zweryfikowanym modelem.

Przyjęcie danych społeczności pozwala używać ich eksperymentalnych parametrów; nie zmienia ich statusu na verified. Brak pamięci gry, injection, automatycznego strzelania, sterowania pojazdem i zapisu klatek gry został zachowany.

Wcześniejsze raporty: [0.2](test-report-0.2.0.md), [0.1](test-report-0.1.0.md). [Stan wymagań](requirements.md).
