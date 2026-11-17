# Przechwytywanie okna — Windows Graphics Capture

Moduł `mtc-capture` przechwytuje wybrane, widoczne okno przez publiczne API Windows. Analiza nie odczytuje pamięci procesu, nie wstrzykuje kodu, nie instaluje hooków i nie zapisuje klatek. Zapis na dysk istnieje wyłącznie jako opcjonalne nagrywanie testowe aplikacji, po jawnej zgodzie (sekcja „Nagrywanie testowe”). Kursor jest wyłączony. Systemowa ramka sygnalizująca przechwytywanie pozostaje włączona.

## Interfejs integracji

```rust
use mtc_capture::{CaptureBackend, WindowCaptureSession};
use std::time::Duration;

let mut capture = WindowCaptureSession::start_with_interval(hwnd, Duration::from_millis(500))?;
let frame = capture.next_frame(Duration::from_secs(2))?;
assert_eq!(frame.backend, CaptureBackend::WindowsGraphicsCapture);
// frame.image: RGBA, tylko obszar klienta, bez paska tytułowego i obramowania.
// frame.captured_at_ms: czas źródłowej klatki, Unix ms.
// frame.source_timestamp_100ns: QPC Windows przeliczony na jednostki 100 ns.
// frame.sequence: rosnący numer klatki w obrębie sesji.
capture.set_frame_interval(Duration::from_millis(333));
capture.stop()?;
```

`WindowCaptureSession::start(handle)` ustawia domyślnie 333 ms między kopiami GPU → CPU. `start_with_interval` i `set_frame_interval` ograniczają rzeczywisty odczyt GPU, a nie tylko częstotliwość OCR. Przedział jest ograniczony do 16–10000 ms. Przy przedziale dłuższym niż timeout wywołujący musi zwiększyć czas oczekiwania. `handle()` zwraca wybrany uchwyt. Sesję można przenieść pomiędzy wątkami (`Send`); `next_frame` wymaga wyłącznego dostępu. Wszystkie obiekty WinRT i D3D11 obsługuje dedykowany wątek.

`capture_window_with_backend(handle, backend)` wykonuje pojedyncze przechwycenie i oddaje `CapturedFrame`. W cyklicznej analizie należy zachować sesję. `CaptureBackend` zapisuje się w JSON jako `"wgc"` albo `"gdi"`. `capture_window(handle)` pozostaje zgodnym ze starszym kodem, jawnym backendem GDI. Błąd WGC nigdy nie uruchamia GDI automatycznie.

## Aktualność, geometria i zasoby

Pula systemowa ma dwa bufory GPU. Gdy oba czekają w kolejce, kompozytor nie dostarcza nowych klatek. Dlatego między kopiami wątek przy każdym `FrameArrived` zwraca do puli starsze powierzchnie i trzyma tylko najnowszą, bez kopiowania GPU; jeden bufor zawsze pozostaje wolny. Wcześniej kopia przy limicie 0,25–1 FPS czytała powierzchnię zakolejkowaną tuż po poprzedniej kopii — tak starą jak interwał — a limit świeżości 1 s odrzucał każdą kolejną klatkę (`Timeout`; przy domyślnych 333 ms klatka miała ~300 ms opóźnienia). Powierzchnia starsza niż limit jest teraz odrzucana przed kopią GPU, bez czekania na pełny kolejny interwał. Wątek przechowuje najwyżej jeden najnowszy obraz CPU. Konsument otrzymuje tylko sekwencję nowszą od poprzednio odebranej. Wiek źródła jest obliczany z `SystemRelativeTime` oraz QPC, a nie z czasu wywołania OCR. Klatka starsza niż sekundę, duplikat lub obraz o rozmiarze niezgodnym z aktualnym obszarem klienta są odrzucane. Bez nowej klatki `next_frame` kończy się błędem `Timeout`.

Dokumentacja Microsoft definiuje [`SystemRelativeTime` jako czas QPC kompozytora](https://learn.microsoft.com/en-us/uwp/api/windows.graphics.capture.direct3d11captureframe.systemrelativetime?view=winrt-26100). Każdy źródłowy timestamp większy od bieżącego QPC jest blokowany, również o pojedynczą jednostkę 100 ns. Powierzchnia z wyprzedzeniem nie większym niż 200 ms może pozostać w ograniczonym buforze oczekiwania, bez odczytu GPU. Jest użyta dopiero po rzeczywistym dogonieniu źródłowego czasu przez QPC; metadane zachowują oryginalny timestamp. Wyprzedzenie ponad 200 ms albo przekroczenie czasu oczekiwania powoduje odrzucenie powierzchni i oczekiwanie na następną, bez zamykania sesji. Brak prawidłowej klatki nadal kończy wywołanie `Timeout`. Nie stosujemy przesuwania przyszłego pomiaru do „teraz”.

Przy opróżnianiu puli obie pobrane powierzchnie pozostają żywe do zakończenia odczytu wybranej. Deferred frames są zwracane do puli przed jej zamknięciem. Kontekst D3D11 ma włączoną [ochronę wielowątkową](https://learn.microsoft.com/en-us/windows/win32/api/d3d11_4/nn-d3d11_4-id3d11multithread) oraz blokadę wokół kopii/mapowania. Fabryki WinRT dla `IsSupported` i `CreateFreeThreaded` mają czas życia sesji/apartamentu zamiast globalnego cache przeżywającego `RoUninitialize`.

ROI odnosi się zawsze do `(0, 0)` obszaru klienta. Pozycja i rozmiar są pobierane w fizycznych pikselach przez tymczasowy, lokalny kontekst DPI. Przed wycięciem porównywane są wymiary źródła z DWM extended frame bounds i pełnym prostokątem okna. Kod nie zgaduje wysokości paska tytułowego. Przejściowe rozmiary podczas zmiany okna są odrzucane; pula jest odtwarzana po zmianie `ContentSize`. Bufor staging jest używany ponownie, a padding wierszy GPU nie trafia do obrazu RGBA.

Zamknięcie jest wykrywane przez zdarzenie `GraphicsCaptureItem.Closed` i `IsWindow`. Ukrycie, minimalizacja i DWM cloaking natychmiast blokują zwracanie zapamiętanego obrazu. Po powrocie z ukrycia lub minimalizacji sesja WGC jest odtwarzana, ponieważ na części konfiguracji DWM zachowuje czarną powierzchnię poprzedniej sesji. `stop()` i `Drop` usuwają handlery zdarzeń, zwracają przetrzymywane powierzchnie, zamykają sesję i pulę oraz zwalniają GPU. Jeżeli wątek WGC zakończył pracę (np. błąd urządzenia lub panika), `next_frame` od razu zwraca błąd zamiast kolejnych `Timeout`. `CaptureError::requires_session_restart()` wskazuje błędy, po których sesji nie da się wznowić (`Closed`, `Stopped`, `Unsupported`, `InvalidSize`, `Backend`, np. utrata urządzenia GPU); aplikacja porzuca wtedy sesję i przy następnym żądaniu tworzy nową. Ukrycie, minimalizacja, zmiana geometrii i `Timeout` zachowują sesję. Konwersja BGRA → RGBA wykonuje jedną alokację na klatkę i nie wymaga paddingu po ostatnim wierszu mapowanego bufora. Uruchamianie ma limit pięciu sekund; nie można bezpiecznie przerwać zawieszonego wywołania sterownika GPU.

## Nagrywanie testowe (aplikacja)

Komendy `recording_start`, `recording_stop`, `recording_status`, `recording_load` i `analyse_recording` (`apps/desktop/src-tauri/src/recording.rs`) zapisują sekwencję klatek wybranego okna do katalogu wskazanego w oknie dialogowym. Wymagają `consent: true` z pola wyboru w panelu **Analiza obrazu**; pole jest domyślnie odznaczone, nie trafia do ustawień i jest czyszczone po każdym nagraniu oraz po zmianie okna lub backendu. Nagrywanie używa osobnej sesji WGC z interwałem równym limitowi FPS (GDI: kolejne pojedyncze przechwycenia z własną numeracją sekwencji), więc działa także bez uruchomionego OCR. Kończy się po kliknięciu **Zatrzymaj**, osiągnięciu limitu klatek/rozmiaru/czasu, zamknięciu okna źródłowego lub opuszczeniu panelu. Ukryte i zminimalizowane okno nie jest zapisywane. Podczas nagrywania panel pokazuje czerwony wskaźnik „NAGRYWANIE” (stały znacznik w prawym górnym rogu oraz pasek z liczbą klatek, rozmiarem, czasem i wykorzystaniem limitów). Format pliku i analizę offline opisuje [vision.md](vision.md).

## Weryfikacja

Październik 2026: `cargo test -p mtc-capture` — 14 testów jednostkowych i 2 integracyjne zakończone powodzeniem. Integracja WGC sprawdza dodatkowo sesję 0,67 FPS (interwał 1,5 s): kolejne klatki muszą mieć wiek ≤ 400 ms. Z wyłączonym zwracaniem powierzchni i dawnym kopiowaniem przeterminowanej powierzchni ten sam test kończył się `Timeout`. Ręczny przebieg na oknie `hud_fixture` nagrał 4 klatki PNG co ~500 ms przez WGC i odtworzył je przez OCR (`27:42` w 3 klatkach, 1 stabilna; jedna klatka uchwyciła biały, przemalowywany stan okna testowego i została poprawnie zgłoszona jako pusta).

Wcześniej wykonano na lokalnym, interaktywnym pulpicie Windows:

```powershell
cargo test -p mtc-capture --all-targets -- --test-threads=1
cargo clippy -p mtc-capture --all-targets -- -D warnings
cargo build -p mtc-capture --example hud_fixture
```

Wynik: 11 testów jednostkowych i 2 testy integracyjne przeszły; Clippy bez ostrzeżeń. Regresje czasu sprawdzają odrzucenie przyszłości o jeden tick, zachowanie oryginalnego źródła po dogonieniu QPC oraz limit 200 ms. Integracja WGC tworzy własne okno Win32 i sprawdza dwa znane kolorowe punkty po wycięciu obszaru klienta, rzeczywiste źródłowe timestampy, rosnącą sekwencję, zmianę rozmiaru, minimalizację, ukrycie, stop/restart i zamknięcie. Test GDI synchronizuje namalowany obszar z DWM. Testy nie przechwytują gry ani aktywnego okna użytkownika. Po `ShowWindow`/resize możliwa jest przejściowa pusta powierzchnia; test oczekuje na zakończenie malowania własnego okna, zachowując limit czasu.

```powershell
target/debug/examples/hud_fixture.exe
target/debug/examples/hud_fixture.exe --multi-hud --passive --seconds 60
```

Domyślny helper zachowuje biały HUD z `27:42` w obszarze klienta `(40,50)`, 64 px. Wariant `--multi-hud` dodaje `138` w `(40,130)`, nazwę `BM-21 Grad` w `(40,190)` i teksturowaną mapę `(270,240,350,200)`. Zielony punkt własny ma środek `(316,276)`, czerwony cel `(506,376)`; oba mają 12×12 px. Względem mapy centra wynoszą `(46,36)` i `(236,136)`. Róg poza obszarem OCR zmienia się co 100 ms, aby powstawały kolejne źródłowe klatki. Pierwsza linia stdout to sam uchwyt HWND. Domyślnie helper kończy pracę po 45 s. `--passive` wyłącza aktywację okna i nie wysyła klawiszy.

### Regresja czasu i ponownego uruchamiania sesji

```powershell
cargo build -p mtc-capture --examples
target/debug/examples/wgc_clock_probe.exe --iterations 40
target/debug/examples/wgc_clock_probe.exe --iterations 40 --restart
```

Probe tworzy własny pasywny helper i sprawdza jego PID przed przechwytywaniem. Wykonany początkowy przebieg trwałej sesji miał 80 klatek / 0 błędów. Pierwszy test wielokrotnego start/stop ujawnił AV `0xC0000005`; zgodny PDB ostatniego nieudanego przebiegu wskazał `GraphicsCaptureSession::IsSupported` przy następnym uruchomieniu. Zastąpienie globalnego cache przez fabryki lokalne dla apartamentu usunęło ten błąd w wykonanym przebiegu 40 restartów / 80 klatek / 0 błędów, exit 0. Dowody: `tests/output/wgc-clock-crash-events-before.txt` oraz `tests/output/wgc-clock-probe-churn-after.log`.

Churn odtworzył także źródła wyprzedzające QPC o około 11–13 ms. Przykład: `qpc_now=132387452842`, `qpc_frequency=10000000`, `source_timestamp_100ns=132387570751`, `delta_100ns=-117909`. Po odczekaniu zwrócona klatka zachowała źródło `132387570751`. To obserwacja lokalnego testu, nie dowód ogólnej przyczyny w Windows ani potwierdzenie aktualnego MTC. Nie rejestrowano obrazu i nie używano Computer Use ani klawiszy.

Przyszłe klatki mają diagnostykę `WGC_CLOCK_RETRY` ograniczoną do jednej linii na sekundę/sesję. Ustawienie `MTC_CAPTURE_CLOCK_DIAGNOSTICS=1` w środowisku procesu dodatkowo loguje QPC, częstotliwość, źródłowy timestamp i różnicę dla każdej weryfikacji. Log zawiera liczby czasu, bez klatek.

## Ograniczenia i źródła

WGC wymaga obsługi przez Windows, aktywnego pulpitu i sterownika D3D11. Na ekranie HDR pula w formacie BGRA8 dostaje obraz przekonwertowany przez system do SDR; jasność i kontrast HUD mogą się różnić od SDR i wymagają sprawdzenia w grze. Podgląd, kalibracja mapy i wybór markerów nadal uruchamiają krótką, osobną sesję WGC na jedną klatkę (koszt startu D3D i mignięcie ramki). Wyłączenie kursora wymaga Windows 10 2004 lub nowszego; brak obsługi daje jawny błąd. Działa przechwytywanie SDR BGRA8. HDR może wymagać przyszłego tone mappingu. Ochrona przed przechwytywaniem, utrata urządzenia GPU, sesja zdalna lub ekran blokady mogą powodować błąd albo brak nowych klatek. Gra może renderować chwilowo pusty obraz; moduł nie interpretuje czerni jako potwierdzonego HUD i nie wymyśla pomiarów. Rzeczywisty Roblox/MTC, HDR i fizyczne przenoszenie okna między monitorami o różnym DPI wymagają osobnej weryfikacji. GDI przechwytuje ekran i zawiera zasłaniające okna; WGC przechwytuje wybrane okno.

Implementacja korzysta z istniejącego `windows` 0.61 i API Microsoft. Sprawdzono [windows-capture 2.0.1](https://github.com/NiiightmareXD/windows-capture) oraz jego [licencję MIT](https://github.com/NiiightmareXD/windows-capture/blob/main/LICENCE). Biblioteka nie została dodana ani skopiowana: wymaga innej wersji `windows` i włącza niepotrzebne moduły kodowania multimediów.

Dokumentacja API: [przechwytywanie i zmiana rozmiaru](https://learn.microsoft.com/en-us/windows/apps/develop/media-authoring-processing/screen-capture), [CreateForWindow](https://learn.microsoft.com/en-us/windows/win32/api/windows.graphics.capture.interop/nf-windows-graphics-capture-interop-igraphicscaptureiteminterop-createforwindow), [CreateFreeThreaded](https://learn.microsoft.com/en-us/uwp/api/windows.graphics.capture.direct3d11captureframepool.createfreethreaded), [DwmGetWindowAttribute](https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/nf-dwmapi-dwmgetwindowattribute), [SystemRelativeTime](https://learn.microsoft.com/en-us/uwp/api/windows.graphics.capture.direct3d11captureframe.systemrelativetime).
