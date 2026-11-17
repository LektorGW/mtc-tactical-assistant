# Kalibracja i obsługa 0.2

Kalibracja łączy obraz, skalę, jednostki i konkretny profil pocisku. Brak dowolnego wymaganego wejścia ma zatrzymać odpowiednią predykcję. Symulacja jest punktem kontrolnym działania aplikacji, bez odniesienia do parametrów MTC.

## Mapa, skala i jednostki

Najprostsza droga to tryb **Mapa z katalogu**: wybór mapy ustawia skalę `sizeGameMeters / szerokość tekstury`, północ u góry i metry gry. Działo stawiasz lewym, cel prawym przyciskiem myszy; punkty można przeciągać, a mapę przybliżać kółkiem. Współrzędne kliknięcia są przeliczane na piksele naturalnego obrazu, niezależnie od DPI i zoomu. Szczegóły: [mapy i teren](auto-maps.md).

W trybie **Własny obraz (zaawansowane)** mapa musi być płaskim obrazem bez perspektywy. Wskaż dwa różne punkty A/B o znanym dystansie; skala to dystans podzielony przez długość odcinka w pikselach oryginalnego obrazu. Model zakłada jednorodną skalę. Góra obrazu ma domyślnie azymut 0°; zmień go dla obróconej mapy. Import mapy albo zapamiętanie nowego widoku usuwa poprzednią skalę i punkty.

Dla własnego obrazu całej mapy możesz wybrać jej nazwę z katalogu i potwierdzić, że obraz obejmuje całą kwadratową mapę (wymaga jednorazowej akceptacji źródła). **Użyj wymiaru źródłowego** ustawia skalę w metrach gry. Nazwa w katalogu nie rozpoznaje bieżącej mapy, a jej pełny wymiar nie pasuje automatycznie do fragmentu minimapy. [Pochodzenie katalogu](community-sources.md).

Dystans, różnica wysokości, prędkości i grawitacja muszą używać zgodnych jednostek: `game_units` albo `game_meters`. Zmiana podpisu jednostki w UI nie przelicza wartości. Prędkość/wysokość z OCR nie potwierdzają zgodności z jednostkami profilu. Niepewność dotyczy każdej pozycji; sumowany limit odcinka jest dwukrotnością tej wartości i nie ma znaczenia probabilistycznego. Zero oznacza brak oszacowania.

## Pozycje ręczne i widoczne markery

W Manual/Assisted punkty można wskazać na mapie albo wpisać w pikselach obrazu. Po ruchu stanowiska lub celu trzeba je zaktualizować. Pozycje z obrazu są opcjonalne; do ich uruchomienia:

1. W **Analiza obrazu** wybierz widoczne okno, pobierz klatkę i zaznacz ROI pełnej mapy.
2. Kliknij **Zapamiętaj widok mapy i użyj go jako referencji**. Wybrany wycinek trafia do mapy roboczej, a mały deskryptor widoku do ustawień; pełna klatka nie jest zapisywana. Ustaw następnie skalę oraz kierunek tej referencji.
3. Włącz **Analizuj markery widoczne w obszarze mapy**, potwierdź zgodność kadru i ustaw rzeczywiste kolory oraz progi ikon dla własnej pozycji i celu. Role określasz sam; kolor nie rozstrzyga automatycznie drużyny.
4. W Assisted zatwierdź jednoznaczne punkty przyciskiem **Przyjmij jednoznaczne punkty w Assisted**. Automatic wymaga aktualnej referencji, powtórzenia stabilnego odczytu i jednego markera każdej roli.

Brak tekstury, zmiana mapy/zoomu/rotacji, przycięcie ikony albo wiele kandydatów blokuje automatyczne pozycje. Po zmianie źródła lub kalibracji wcześniejsze obserwacje są unieważniane. Referencja mapy nie jest automatycznym rozpoznaniem współrzędnych świata; jej skala i orientacja pozostają odpowiedzialnością użytkownika. [Szczegóły markerów i deskryptora](vision.md).

Tracker może estymować ruch własnej pozycji i celu z kolejnych świeżych markerów. Ustaw **Limit odrzucania skoków trackera** zgodnie z jednostką mapy. Stan collecting/stale/rejected nie dostarcza prędkości; ready nadal opiera się na przybliżeniu stałej prędkości. Historia jest resetowana po zmianie źródła, konfiguracji lub kalibracji, a zniknięcie/nakładanie markerów wstrzymuje ruch.

## Profil pocisku i ruch stanowiska

W **Profile i balistyka** wpisz parametry zmierzone w grze: prędkość początkową, stałą grawitację, źródło, opcjonalny rozrzut i rozmiar salwy. Zapisz nazwę amunicji, wersję gry, datę i opis pomiaru. Wynik lokalnej kalibracji ma status beta; porównaj go z trafieniami. Limity elewacji profilu są edytowane w JSON.

Dane społecznościowe można wypróbować po świadomym potwierdzeniu konkretnego pocisku; wymagają metrycznej konwencji źródła i nie stają się przez to zweryfikowane. Brak parametrów, niezgodne jednostki i kategoria guided blokują predykcję. [Schemat i bramka profilu](weapon-profiles.md).

Prędkości celu oraz stanowiska wpisuj w osiach **wschód / północ**. W Automatic dostępny tracker może zastąpić ręczne prędkości, gdy obserwacje są świeże i jego stan ready. Dziedziczenie ruchu własnego wymaga zaznaczenia **Model wyprzedzenia dziedziczy prędkość stanowiska**. Aplikacja używa wtedy prędkości względnej; zgodność tego założenia z grą wymaga testów.

Wyprzedzenie jest obliczeniem prostoliniowym 2D **bez grawitacji** i z ograniczonym horyzontem predykcji. Nie jest zintegrowanym rozwiązaniem toru balistycznego dla poruszającego się śmigłowca. Niski/wysoki tor liczy oddzielny solwer stałej grawitacji bez drag/napędu. Różnica wysokości pochodzi z mapy wysokości, gdy jest wczytana dla bieżącej mapy i tej samej geometrii skali (niepewność pozycji nie ma znaczenia). Inaczej użyj pola ręcznego. Wynik podaje źródło wysokości. Brak próbki w punkcie nie jest zastępowany zerem. Heightmapa nie podaje wysokości celu powietrznego.

## Korekta i historia

Wskazanie trafienia i korekta przesuwa obecny punkt celowania o wektor przeciwny do błędu: nowy punkt = 2 × obecny cel − trafienie. To ręczna korekta geometryczna; nie zmienia parametrów pocisku, nie wykonuje strzału i nie może wyprowadzić punktu poza mapę. W Automatic ręczne wskazanie pozycji przełącza na Assisted.

**Zapisz rozwiązanie** zachowuje wyliczenia i opis, bez twierdzenia, że salwa została oddana. Rozrzut 95% dotyczy pojedynczego pocisku w przyjętym modelu; brak wartości nie potwierdza idealnej celności.

## ROI i aktualność

ROI jest podany jako X/Y/szerokość/wysokość z zakresu 0–1 względem obszaru klienta wybranego okna. Pobierz klatkę po zmianie proporcji i zaznacz każdy używany obszar oddzielnie. Kompas powinien obejmować jedną centralną liczbę. Jawne nazwy są tekstem, bez identyfikacji po sylwetce. Automatic wymaga świeżych stabilnych odczytów i heurystycznego progu 85%.

Limit FPS ogranicza także odczyt GPU WGC; domyślny backend nie przechodzi na GDI automatycznie. Okno ukryte, zminimalizowane lub zamknięte nie dostarcza zapamiętanej klatki. Klatki i podgląd pozostają w pamięci. Aktualny HUD/mapa MTC, fizyczne zmiany DPI i mechanika pocisków wymagają osobnej weryfikacji; kalibracja nie steruje grą.

