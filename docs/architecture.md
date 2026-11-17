# Architektura 0.3

Panel główny i osobne okno nakładki korzystają z lokalnego interfejsu Tauri 2 / React / TypeScript. Rust obsługuje publiczne API Windows, OCR, kalibrację mapy, walidację profili i obliczenia. UI prezentuje oddzielnie dane ręczne, odczyty ekranu oraz wyniki modelu. Nakładka ma przezroczysty WebView2, always-on-top i przełączane przepuszczanie kliknięć; globalne skróty działają niezależnie od fokusu WebView.

~~~mermaid
flowchart TD
    Window[Wybrane widoczne okno] --> Capture[WGC: obszar klienta i źródłowy czas]
    Capture --> Latest[Jedna najnowsza klatka]
    Latest --> OCR[Pełnoklatkowy OCR i wykrycie HUD albo ręczne ROI]
    Latest --> MapView[Lokalne mapy i dopasowanie geometrii albo stały widok]
    MapView --> Markers[Widoczne komponenty o wybranych rolach]
    Markers --> Tracker[Świeże pozycje i estymacja ruchu]
    Manual[Dane i kalibracja ręczna] --> Maps[Skala, orientacja, dystans i azymut]
    Tracker --> Maps
    Maps --> Terrain[Lokalna mapa wysokości lub wysokość ręczna]
    Terrain --> Ballistics[Model bez oporu i przechwycenie 3D]
    Profile[Lokalna kalibracja lub potwierdzony profil eksperymentalny] --> Ballistics
    OCR --> UI[Panel i nakładka]
    Ballistics --> UI
~~~

## Przechwytywanie i analiza

W 0.3 `automation.rs` łączy layout OCR, stabilizację położenia pól, resolver pojazdu/amunicji oraz bibliotekę referencji map. Automatyczne pomiary korzystają z tekstu pełnoklatkowego rozpoznania tej samej klatki. ID, rodzaj, ROI, wymiary, źródłowy czas oraz dowody propozycji muszą pasować. Nadal wymagane są dwie różne świeże obserwacje wartości.

Biblioteka map wiąże kalibrację z wymiarami i SHA256 lokalnego pliku. Dopasowanie cech całej klatki daje transformację klient → referencja, w której przeliczane są centroidy widocznych markerów. Jawna rozpoznana nazwa przecząca znanej referencji wstrzymuje pozycje. [Kontrakt map](auto-maps.md).

`capture` domyślnie używa Windows Graphics Capture: WinRT `CreateForWindow`, wolnowątkowa pula D3D11, publiczne zdarzenia i kopiowanie obszaru klienta GPU → CPU. Pula zawiera dwa bufory GPU, a worker najwyżej jeden najnowszy obraz CPU. Interwał analizy ogranicza rzeczywisty odczyt GPU. Staging texture jest używany ponownie; rozmiar puli zmienia się wraz z `ContentSize`.

Wycięcie klienta korzysta z fizycznych współrzędnych DPI oraz granic DWM, bez stałej wysokości paska tytułowego. Kursor jest wyłączony. Źródłowe `SystemRelativeTime` oraz QPC określają wiek klatki; duplikaty, niezgodne wymiary i źródło starsze niż sekundę są odrzucane. Zamknięcie/ukrycie/minimalizacja blokują retained image. Powrót z ukrycia/minimalizacji odtwarza pipeline. GDI pozostaje jawnym fallbackiem, bez automatycznego przełączania; obejmuje przesłaniające okna. [Szczegóły capture](capture.md).

`vision` obsługuje do 16 niezależnych, normalizowanych ROI jednej klatki. Windows.Media.Ocr dostarcza tekst, który jest walidowany jako timer, pojedynczy kompas, wysokość, prędkość albo jawny napis. Kilka liczb w kompasie blokuje odczyt. Nazwa nie jest rozpoznaniem modelu po wyglądzie; opcjonalny wybór profilu w UI wymaga jednoznacznej zgodności napisu z katalogiem.

Wariant detektora ze stałym ROI wymaga wybranego pełnego kadru, potwierdzenia geometrii i zapamiętanego deskryptora 16×16. Porównuje teksturę/jasność i gradienty bieżącego widoku z referencją. Nie ustala semantycznej nazwy ani skali mapy. Dopasowanie jest heurystyką; obrazy pozbawione użytecznej tekstury są odrzucane. Wykrywanie markerów używa spójnych komponentów o wybranych kolorach, wielkości i kształcie, do ośmiu sygnatur. Role `own` i `target` określa użytkownik; kilka kandydatów wstrzymuje automatyczne pozycje.

Tracker przechowuje ograniczoną historię świeżych pozycji w skalibrowanej płaszczyźnie mapy i dopasowuje prędkość stałą. Zmiana okna, konfiguracji, mapy lub kalibracji zeruje obserwacje. Brak markera, niski poziom pewności i skoki blokują estymację. Analiza plików offline nie dostarcza bieżącej telemetrii do Automatic. [Konfiguracja vision i trackera](vision.md).

## Obliczenia i profile

Resolver rozdziela pojazd i amunicję. Sam pojazd pozostaje niejednoznaczny, nawet gdy katalog zna jeden pocisk. Dopasowanie nazwy nie potwierdza źródła parametrów. Kalibracja z pomiarów czasu, dystansu, elewacji i wysokości dopasowuje model stałej prędkości/grawitacji, sprawdza uwarunkowanie oraz reszty i otrzymuje status beta. Przechwycenie 3D uwzględnia grawitację i jawny model dziedziczenia prędkości. Brak wysokości zachowuje bearing, ale nie tworzy trajektorii.

Heightmap jest liniowym, potwierdzonym obrazem szarości w jednostkach konkretnej mapy. Obliczenia wymagają zgodności bieżącego ID i kalibracji; brak próbki lub punkt poza zakresem blokuje interpolację. Mapa terenu nie określa wysokości celu powietrznego nad ziemią. [Kontrakt modelu](auto-weapons.md).

`maps` transformuje piksele do płaskich współrzędnych świata przez skalę, obrót i punkt początkowy. Azymut ma północ 0° i wschód 90°. Skala pochodzi z ręcznego odcinka albo jawnie zaakceptowanego pełnego wymiaru katalogowej mapy; konwersja nie rozpoznaje minimapowego zoomu.

`profiles` waliduje wersję schematu, jednostki, parametry, status i pochodzenie. Katalog zawiera siedem szablonów oraz 153 profile pocisków dla 51 nazw uzbrojenia z przypiętego źródła społeczności. Osobny katalog ma wymiary 25 map. Profile społeczności są domyślnie `experimental`, nieskalibrowane i niepotwierdzone. Solwer dopuszcza lokalną kalibrację albo potwierdzenie konkretnego pocisku przez `sourceAcknowledged`. Potwierdzenie nie zmienia statusu na verified. Jednostki profilu i mapy muszą być zgodne; `game_meters` jest konwencją źródłowego kalkulatora.

`ballistics` pozostaje czystą biblioteką niezależną od capture/OCR/UI. Oblicza dwa tory w stałej grawitacji bez oporu, czas lotu, różnicę wysokości i ograniczone wyprzedzenie. Opcjonalna prędkość stanowiska jest jawnym przybliżeniem dziedziczenia ruchu; sama estymacja markera nie potwierdza tej mechaniki MTC. Profil `guided_missile` ma wyłączony solwer również po potwierdzeniu źródła. Brak modelu napędu i drag; wysokość terenu wymaga zgodnej lokalnej heightmap.

## Sterowanie przepływem i dane lokalne

Komendy Tauri wykonują kosztowną analizę na workerach. Trwała sesja WGC jest przypisana do uchwytu okna; capture/analiza mają pojedynczy aktywny worker, a frontend planuje następne żądanie po poprzednim. Zatrzymanie i zmiana źródła unieważniają poprzedni wynik. Świeżość jest sprawdzana przed OCR, po nim oraz przed publikacją i obliczeniami. UI i nakładka nie prezentują pomiarów starszych niż 2 s. Automatic wymaga stabilizacji i progu heurystycznego 85%; brak jednoznacznych pozycji wstrzymuje rozwiązanie.

CSP blokuje zewnętrzne skrypty, ramki i obrazy; komunikacja odbywa się przez lokalne IPC. Podgląd klatki jest PNG data URL w pamięci. Zapis ustawień jest wersjonowany i atomowy przez plik tymczasowy oraz rename. Lokalne dane obejmują profile, konfigurację ROI, skalę, punkty, mały deskryptor mapy, historię i log kalibracji. Pełne klatki gry nie są zapisywane; mapa pobrana z kadru nie jest trwałym plikiem.

Snapshot danych społeczności jest dołączony do aplikacji wraz z manifestem SHA256, commitem i licencją MIT. Rozgrywka nie wymaga GitHub ani połączenia sieciowego. Nie ma klienta chmury, odczytu pamięci Roblox, injection ani API sterowania/firing. Zapis klatek na dysk istnieje tylko jako nagrywanie testowe po jawnej zgodzie ([capture](capture.md#nagrywanie-testowe-aplikacja)). Walidacja symulacji i własnych okien nie zastępuje kalibracji aktualnego MTC.


Wersja ustawień 3 przechowuje bibliotekę ścieżek, SHA256, kalibracje i opcjonalny teren. Migracje v1/v2 nie włączają samoczynnie nowych funkcji. Epoch przechwytywania oraz wspólna blokada cyklu życia uniemożliwiają przywrócenie historii przez zatrzymaną klatkę. Generacja ładowania zasobów odrzuca wynik starszego żądania. Frontend wiąże batch z konfiguracją źródła; świeżość porównuje do czasu rzeczywistego przy każdym renderze.
